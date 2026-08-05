//! The run loop and top-level message scheduling.
//!
//! `run_event_loop` wires together:
//!   - crossterm terminal events  -> `AppMsg`
//!   - the central `update`        -> intents + effects
//!   - the `EffectRunner`          -> `Action`s (fed back via a channel)
//!   - the intent router           -> `AppMsg`s (fed back into a queue)
//!   - a periodic tick             -> redraw
//!
//! Cascade handling is **iterative, not recursive**. An update pass may
//! produce `Intent`s that resolve to further `AppMsg`s; those are pushed onto
//! a `VecDeque` and drained in a loop, so arbitrarily deep (or cyclic)
//! cascades never grow the call stack. A per-round depth cap bounds the work
//! done in a single event round so a logic bug cannot turn into an unbounded
//! hot loop (a livelock), though it does not mask an infinite cascade.

use std::collections::VecDeque;
use std::time::Duration;

use crossterm::event::{Event as CEvent, EventStream, KeyCode};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::app::action::Action;
use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app::update::{handle_action, update, UpdateResult};
use crate::app::view::render;
use crate::app_shell::effect::EffectRunner;
use crate::app_shell::intent::IntentRouter;

const TICK_RATE: Duration = Duration::from_millis(250);
/// Upper bound on how many messages are processed in a single event round
/// (the triggering event + the cascade it starts). Guards against livelock
/// caused by a cyclic intent cascade; it does not otherwise change behavior.
const MAX_MESSAGES_PER_ROUND: usize = 100;

/// Run the application. Sets up the terminal, the channels and the event
/// loop, then tears the terminal down on exit.
pub async fn run_event_loop() -> anyhow::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<Action>();
    let effect_runner = {
        let (runner, handle) = EffectRunner::new(action_tx.clone());
        tokio::spawn(handle.run());
        runner
    };

    let mut state = AppState::default();
    let mut reader = EventStream::new();
    let mut tick = tokio::time::interval(TICK_RATE);

    loop {
        // Draw the current frame.
        terminal.draw(|frame| render(frame, &state))?;

        // Wait for the next input: a terminal event, a tick, or an async
        // action (from an effect).
        tokio::select! {
            maybe_event = reader.next() => {
                // Route quit through the standard message flow so the
                // `ShellMsg::Quit` handler sets the quit flag and any future
                // pre-shutdown cleanup is triggered.
                if let Some(Ok(CEvent::Key(key))) = maybe_event
                    && key.code == KeyCode::Char('q')
                {
                    let msg = AppMsg::Shell(crate::app_shell::msg::ShellMsg::Quit);
                    process_message_round(&effect_runner, &mut action_rx, msg, &mut state);
                }
                // Unrecognized keys are ignored: no-op requires no message.
            }
            _ = tick.tick() => {
                let msg = AppMsg::Shell(crate::app_shell::msg::ShellMsg::Tick);
                process_message_round(&effect_runner, &mut action_rx, msg, &mut state);
            }
            maybe_action = action_rx.recv() => {
                if let Some(action) = maybe_action {
                    process_action_round(&effect_runner, &mut action_rx, action, &mut state);
                }
            }
        }

        if state.should_quit {
            break;
        }
    }

    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    Ok(())
}

/// A round triggered by an external message (keyboard/tick). The seed message
/// and any already-available async actions are queued and drained iteratively.
fn process_message_round(
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    seed: AppMsg,
    state: &mut AppState,
) {
    let mut pending: VecDeque<AppMsg> = VecDeque::new();
    pending.push_back(seed);
    drain_async_actions(action_rx, &mut pending);
    drain_pending(effect_runner, &mut pending, state);
}

/// A round triggered by an async action (an effect result). The action is
/// applied via `handle_action`; its intents/effects are consumed by the same
/// iterative drain as a message round.
fn process_action_round(
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    seed: Action,
    state: &mut AppState,
) {
    let mut pending: VecDeque<AppMsg> = VecDeque::new();
    let result = handle_action(seed, state);
    queue_result(effect_runner, result, &mut pending);
    drain_async_actions(action_rx, &mut pending);
    drain_pending(effect_runner, &mut pending, state);
}

/// Iteratively apply every queued message until the queue is empty or the
/// per-round budget is exhausted. Intents produced by an update resolve to new
/// `AppMsg`s that are pushed back onto the queue, replacing recursion with a
/// heap-backed work list.
fn drain_pending(
    effect_runner: &EffectRunner<Action>,
    pending: &mut VecDeque<AppMsg>,
    state: &mut AppState,
) {
    let mut processed = 0usize;
    while let Some(msg) = pending.pop_front() {
        if processed >= MAX_MESSAGES_PER_ROUND {
            tracing::warn!(
                "message round budget ({MAX_MESSAGES_PER_ROUND}) exhausted; \
                 dropping remaining cascade ({} messages)",
                pending.len()
            );
            break;
        }
        processed += 1;

        let result = update(msg, state);
        queue_result(effect_runner, result, pending);
    }
}

/// Submit an update result's effects to the runner and push each routed intent
/// back onto the pending queue. Intents are programmatic cross-feature
/// requests; the messages they resolve to are applied without the focus guard
/// because delivery is not keyboard input.
fn queue_result(
    effect_runner: &EffectRunner<Action>,
    result: UpdateResult,
    pending: &mut VecDeque<AppMsg>,
) {
    for effect in result.effects {
        effect_runner.submit(effect);
    }
    for intent in result.intents {
        let nested = IntentRouter::route::<AppMsg>(intent);
        pending.push_back(nested);
    }
}

/// Move any already-available async actions out of the channel and into the
/// queue as dispatched messages. Does not await; only drains what is ready so
/// the event loop never blocks on effects.
fn drain_async_actions(
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    pending: &mut VecDeque<AppMsg>,
) {
    loop {
        match action_rx.try_recv() {
            Ok(Action::Dispatch(msg)) => pending.push_back(msg),
            Ok(Action::Shell(crate::app_shell::action::ShellAction::Quit)) => {
                pending.push_back(AppMsg::Shell(crate::app_shell::msg::ShellMsg::Quit));
            }
            // Feature-specific actions are not yet handled; they are dropped
            // rather than panicking so the loop stays resilient. `Shell` has a
            // single `Quit` variant and is already covered above.
            Ok(Action::Header(_))
            | Ok(Action::Explorer(_))
            | Ok(Action::Discover(_))
            | Ok(Action::Iw(_))
            | Ok(Action::Sql(_))
            | Ok(Action::Footer(_))
            | Ok(Action::Perf(_)) => {}
            Err(mpsc::error::TryRecvError::Empty) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => break,
        }
    }
}
