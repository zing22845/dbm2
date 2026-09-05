//! The run loop and top-level message scheduling.
//!
//! `run_event_loop` wires together:
//!   - crossterm terminal events  -> `AppMsg`
//!   - the central `update`        -> intents + effects
//!   - the `EffectRunner`          -> `Action`s (fed back via a channel)
//!   - the intent router           -> `AppMsg`s (fed back into a queue)
//!   - event-driven, on-demand redraw (repaint only when state changed or a
//!     timed refresh is actually due; idle sleeps via `pending()` -> ~0% CPU)
//!
//! Cascade handling is **iterative, not recursive**. An update pass may
//! produce `Intent`s that resolve to further `AppMsg`s; those are pushed onto
//! a `VecDeque` and drained in a loop, so arbitrarily deep (or cyclic)
//! cascades never grow the call stack. A per-round depth cap bounds the work
//! done in a single event round so a logic bug cannot turn into an unbounded
//! hot loop (a livelock), though it does not mask an infinite cascade.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crossterm::event::{Event as CEvent, EventStream, KeyCode};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use tokio::sync::mpsc;

use crate::app::action::Action;
use crate::app::hover::normalize_splitter_tracks;
use crate::app::mouse::clear_active_drags;
use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app::update::{UpdateResult, handle_action, update_unchecked};
use crate::app::view::render;
use crate::app_shell::effect::EffectRunner;
use crate::app_shell::intent::IntentRouter;
use crate::features::global_footer::view as footer_view;
use crate::features::perf_monitor::backend::CountingBackend;

/// The terminal the run loop drives: a cell-change counting backend wrapping
/// crossterm. Only its size is needed here (to hit-test against the layout).
pub(crate) type AppTerminal = Terminal<CountingBackend<CrosstermBackend<std::io::Stdout>>>;

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
        crossterm::event::EnableMouseCapture,
        crossterm::event::EnableFocusChange,
        crossterm::event::EnableBracketedPaste
    )?;
    // Restore the terminal even on an early `?` return or a panic so raw mode /
    // alternate screen never leak out and leave the terminal looking "frozen"
    // and unresponsive to keys.
    let _terminal_guard = TerminalGuard;
    // The backend is wrapped in a `CountingBackend` so the redundancy metric
    // can read how many cells each frame actually changed.
    let backend = CountingBackend::new(CrosstermBackend::new(stdout));
    let mut terminal = Terminal::new(backend)?;

    let (action_tx, mut action_rx) = mpsc::unbounded_channel::<Action>();
    // The composition root creates the shared service bundle once and injects
    // it into the effect runner, which hands it to each effect as it runs.
    let services = crate::common::service::services::Services::new()?;
    let effect_runner = {
        let services = std::sync::Arc::new(services);
        let (runner, handle) = EffectRunner::new(action_tx.clone(), services.clone());
        tokio::spawn(handle.run());
        runner
    };

    let mut state = AppState::default();
    // Cache the terminal size once at startup: the terminal does not reliably
    // emit a Resize event on launch, so without this the persisted
    // horizontal-split percentage would be re-materialized against an
    // incorrect (zero) body height and the explorer's horizontal-scroll clamp
    // would have no width to work with.
    if let Ok(size) = terminal.size() {
        state.term_width = size.width;
        state.term_height = size.height;
    }
    let mut reader = EventStream::new();

    // Populate the explorer tree on startup. Use `update_unchecked` so the
    // message is not dropped by the focus guard: at startup focus is still the
    // header, so a guarded `Explorer(Load)` would be discarded and the tree
    // would stay empty until the explorer gained focus.
    let startup_load = crate::app::update::update_unchecked(
        AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
            crate::features::explorer::msg::ExplorerMessage::Instances(
                crate::features::explorer::instances::msg::InstancesMsg::Message(
                    crate::features::explorer::instances::msg::InstancesMessage::Load,
                ),
            ),
        )),
        &mut state,
    );
    let mut startup_pending = std::collections::VecDeque::new();
    queue_result(&effect_runner, startup_load, &mut startup_pending);
    // Synchronously await the startup load's result so the explorer tree is
    // populated *before* restoring the session (and the first frame renders).
    // Otherwise the tree shows empty on the first paint and the restored
    // expansion/cursor state has no nodes to apply to.
    if let Some(action) = action_rx.recv().await {
        process_action_round(&effect_runner, &mut action_rx, action, &mut state);
    }

    // Restore a previously persisted session (open tabs + focus + tree
    // expansion) after the explorer tree is populated, so the instance
    // expansion and cursor from the last run can be reapplied. Restoring
    // expansion may request lazy loads of the expanded instances' connections;
    // submit those effects now so the subtrees are fetched right after startup.
    match crate::app::session::restore_session(&mut state) {
        Ok(effects) => {
            for effect in effects {
                effect_runner.submit(effect);
            }
        }
        Err(e) => tracing::warn!("failed to restore TUI session: {e}"),
    }

    // Transient mouse-gesture state: the splitter being dragged, the last press
    // (double-click detection), the wheel-debounce key and the spin watchdog.
    // It must survive across events but deliberately stays out of `AppState` —
    // state changes flow through `update`, and these values are pure gesture
    // tracking — so the run loop owns it and hands it to `app::mouse` on every
    // mouse event.
    let mut mouse_interaction = crate::app::mouse::MouseInteraction::default();

    // The hardware caret position placed on the previous frame. The editor's
    // caret is written straight to the terminal (crossterm `MoveTo`) rather
    // than into the ratatui buffer, so moving it changes no cells and the
    // change count stays 0. Tracking it lets the redundancy metric tell a
    // caret-only move apart from a genuinely redundant repaint.
    let mut last_cursor_pos: Option<ratatui::layout::Position> = None;

    // Event-driven, on-demand redraw (mirrors the original dbm "Route B"): the
    // screen is only repainted when a real event/action changed state, or when
    // a timed refresh is actually due. With no timed work pending the app
    // sleeps until a real event arrives (~0% CPU).
    let mut needs_redraw = true;
    // A repaint requested by a timed/forced source (counter decay, discover
    // scan tick) rather than a real event. Such repaints must NOT feed the
    // FPS/waste estimates, so they are tracked separately from `needs_redraw`.
    let mut timed_redraw = false;
    // Watchdog: if the select loop ever spins (e.g. a select branch becomes
    // immediately ready), the loop would burn 100% CPU and freeze keyboard
    // input. Any real event (mouse/key/action) or a redraw resets this counter;
    // exceeding the threshold forces a sleep to break the spin.
    const MAX_IDLE_ITERATIONS: u32 = 64;

    loop {
        // Whether real work (an event/action) asked for a repaint, captured at
        // the top of the loop so the forced counter-decay repaint below (which
        // must NOT feed the FPS/waste estimates) is never mistaken for one.
        let real_redraw = needs_redraw;

        // FPS/waste counters decay to 0 once redraws stop (idle) so they
        // reflect live rates, not a stale peak. The footer only updates on a
        // draw, so when stale we force one repaint — but that repaint must NOT
        // feed the estimates below, or it would show a fake rate from the
        // refresh gap.
        let fps_stale = state
            .perf
            .last_real_frame_elapsed()
            .is_some_and(|l| l > Duration::from_millis(250));
        if fps_stale && (state.perf.fps > 0.0 || state.perf.redundancy_rate > 0.0) {
            // Both counters live or die with redraw activity: no frames drawn
            // means no redundancy to report, so waste goes to 0 in lockstep
            // with fps.
            state.perf.fps = 0.0;
            state.perf.redundancy_rate = 0.0;
            timed_redraw = true;
        }

        if needs_redraw || timed_redraw {
            // Exclude the footer's self-updating fps/waste slot from the
            // changed-cell count so those numbers (which change every frame)
            // don't mark every active frame as "changed" and mask real
            // redundancy.
            let size = terminal.size()?;
            // Refresh each horizontal splitter's last-laid-out track so keyboard
            // `+` / `-` nudges clamp against the live body height (and stop
            // dirtying once the split reaches a boundary).
            normalize_splitter_tracks(&mut state, size);
            let footer_h = footer_view::footer_height(&state.footer, size.width);
            terminal
                .backend_mut()
                .set_exclude_rects(perf_exclude_rects(size, footer_h));
            // Draw the current frame. The perf_monitor feature is passive: the
            // run loop samples each frame here and feeds the smoothed FPS and
            // redundant-redraw ratio from the wrapped backend.
            // `render` reports the focused SQL editor's caret; capture it out of
            // the draw closure (which returns `()`), then place the terminal
            // hardware cursor accordingly (edtui hides its own in-buffer caret).
            let editor_cursor = std::cell::RefCell::new(None);
            let targets_layout = std::cell::RefCell::new(None);
            let history_v_scroll_out = std::cell::RefCell::new(None);
            let results_scroll_out = std::cell::RefCell::new(None);
            terminal.draw(|frame| {
                let (c, t, h, r) = render(frame, &state);
                *editor_cursor.borrow_mut() = c;
                *targets_layout.borrow_mut() = t;
                *history_v_scroll_out.borrow_mut() = h;
                *results_scroll_out.borrow_mut() = r;
            })?;
            let cursor = editor_cursor.into_inner();
            let cursor_pos = cursor.as_ref().map(|c| c.position);
            crate::common::editor::apply_hardware_cursor(cursor)?;
            // Feed back the computed targets layout (scroll offset, viewport)
            // to the state so update handlers can clamp scroll correctly.
            if let Some(info) = targets_layout.into_inner() {
                let targets = &mut state.discover.targets;
                targets.scroll_offset = info.scroll_offset;
                targets.target_viewport = info.viewport;
            }
            // Feed back the discover results viewport start so the next frame's
            // anchor sees the reconciled scroll value (required because the
            // viewport height depends on terminal size, which only the renderer
            // knows).
            if let Some(scroll) = results_scroll_out.into_inner() {
                state.discover.results.scroll = scroll;
            }
            // Feed back the reconciled history v_scroll (viewport start) to
            // the state — required because the viewport height depends on
            // terminal size, which only the renderer knows.
            if let Some(v_scroll) = history_v_scroll_out.into_inner()
                && let Some(tab_idx) = state.sql.sql_tab.active_tab
                && let Some(tab) = state.sql.sql_tab.tabs.get_mut(tab_idx)
            {
                tab.history.list.v_scroll = v_scroll;
            }
            let changed_cells = terminal.backend_mut().last_changed_cells();
            if real_redraw {
                // The editor caret is a *hardware* cursor: it is positioned with
                // a direct crossterm `MoveTo` outside the ratatui buffer, so a
                // caret-only change (e.g. moving the cursor in the SQL editor)
                // legitimately repaints zero cells. Count that as real work
                // rather than redundancy, or every caret move would be scored
                // as a wasted repaint.
                let caret_moved = cursor_pos != last_cursor_pos;
                // A caret-only move is real work even though no cell changed, so
                // score it as one changed cell instead of a wasted repaint.
                let effective_changed = changed_cells.max(usize::from(caret_moved));
                last_cursor_pos = cursor_pos;
                // Debug assertion (non-fatal): a real redraw (one asked for by
                // an event/action, i.e. dirty) that changed zero cells means the
                // repaint was over-broad — a message marked `dirty` without
                // actually changing rendered state. Timed/forced repaints never
                // reach this branch, so this only surfaces the event-driven
                // dirty case.
                if changed_cells == 0 && !caret_moved {
                    tracing::debug!("dirty redraw changed 0 cells (over-broad dirty?)");
                }
                state.perf.record_frame();
                state.perf.record_redundancy(effective_changed);
            } else {
                // Forced counter-refresh repaint: bump the frame timestamp so
                // later real redraws still count, but don't record this frame.
                state.perf.touch_frame();
            }
            needs_redraw = false;
            timed_redraw = false;
        }

        // Wait for the next wake: a terminal event, an async action, or (only
        // when a timed repaint is actually due) a timed refresh.
        tokio::select! {
            maybe_event = reader.next() => {
                if let Some(Ok(CEvent::Key(key))) = maybe_event {
                    use crate::app_shell::msg::ShellMsg;
                    use crossterm::event::KeyModifiers as KM;
                    let ctrl = key.modifiers.contains(KM::CONTROL);
                    let global = match (ctrl, key.code) {
                        // `CTRL+D` quits through the standard message flow so
                        // the handler sets the quit flag. (`q` is no longer a
                        // global quit shortcut.)
                        (true, KeyCode::Char('d')) => {
                            Some(AppMsg::Shell(ShellMsg::Quit))
                        }
                        // `CTRL+T` toggles the active theme (dark/light).
                        (true, KeyCode::Char('t')) => {
                            Some(AppMsg::Shell(ShellMsg::ToggleTheme))
                        }
                        // Not a global shortcut: let the focused feature handle
                        // the key below.
                        _ => None,
                    };
                    let msg = global.or_else(|| crate::app::input::key_to_msg(key, &state));
                    if let Some(msg) = msg {
                        // Repaint only if the round actually changed rendered
                        // state (dirty); an input dropped by the focus guard, or
                        // a no-op key, skips the redraw.
                        let result =
                            process_message_round(&effect_runner, &mut action_rx, msg, &mut state);
                        needs_redraw |= result.dirty;
                    }
                    // `needs_redraw` stays as-is for a key that produced no
                    // message: it never changes state, so nothing to repaint
                    // unless a timed refresh (e.g. FPS counter decay) already
                    // asked for one above.
                } else if let Some(Ok(CEvent::Mouse(mouse))) = maybe_event {
                    // Mouse input lives in `app::mouse`: hit-test the position
                    // against the rendered layout, then dispatch the resulting
                    // messages through `update`.
                    match crate::app::mouse::handle_mouse_event(
                        mouse,
                        &mut terminal,
                        &mut state,
                        &effect_runner,
                        &mut action_rx,
                        &mut mouse_interaction,
                    )? {
                        // A debounced wheel tick: skip the rest of this loop
                        // iteration, exactly as the inlined `continue` did.
                        crate::app::mouse::MouseOutcome::Continue => continue,
                        crate::app::mouse::MouseOutcome::Handled { repaint } => {
                            needs_redraw |= repaint
                        }
                    }
                } else if let Some(Ok(CEvent::Paste(contents))) = maybe_event {
                    // Bracketed paste: route the pasted text to the focused
                    // editor cell. The discover targets editor and the SQL
                    // editor both accept it (TSV host:ports rows / text).
                    let msg = crate::app::input::paste_to_msg(&contents, &state);
                    if let Some(msg) = msg {
                        let result =
                            process_message_round(&effect_runner, &mut action_rx, msg, &mut state);
                        needs_redraw |= result.dirty;
                    }
                } else if let Some(Ok(CEvent::Resize(w, h))) = maybe_event {
                    // Terminal window resized: force a repaint so the layout
                    // recomputes against the new terminal size. Without this,
                    // `terminal.draw` is skipped while idle (no dirty state) and
                    // the rendered frame never catches up with the window size,
                    // unlike the original dbm which redraws on resize.
                    // Also update the cached terminal size so the explorer's
                    // horizontal-scroll can clamp at the content boundary and
                    // the persisted horizontal-split percentage can be
                    // re-materialized against the new body height.
                    state.term_width = w;
                    state.term_height = h;
                    needs_redraw = true;
                } else if let Some(Ok(CEvent::FocusLost)) = maybe_event {
                    // The terminal window lost focus — e.g. the user dragged a
                    // scrollbar (or splitter) and released the mouse button
                    // *outside* the window, so no `MouseEventKind::Up` is ever
                    // delivered. Any held-button drag state would otherwise stay
                    // "active" forever (the scrollbar thumb keeps its accent
                    // color). Clear every in-progress drag here so the UI resets.
                    let was_dragging = state.scrollbar_drag.is_some()
                        || mouse_interaction.splitter_drag.is_some()
                        || state.splitter_hover.results_col_resize_drag.is_some()
                        || state.splitter_hover.dragging_flags().iter().any(|&f| f);
                    if was_dragging {
                        mouse_interaction.splitter_drag = None;
                        clear_active_drags(&mut state);
                        needs_redraw = true;
                    }
                }
            }
            // Timed refresh: only fires when a periodic repaint is actually due
            // (discover scan progress, counter decay). With nothing timed
            // pending, `next_wake` returns `None` and this branch sleeps
            // forever until a real event arrives.
            _ = sleep_until_opt(next_wake(&state)) => {
                // Tracked separately so the repaint feeds `touch_frame`, not
                // the FPS/waste estimates.
                timed_redraw = true;
            }
            maybe_action = action_rx.recv() => {
                if let Some(action) = maybe_action {
                    // Repaint only if the effect result changed rendered state.
                    let result = process_action_round(
                        &effect_runner,
                        &mut action_rx,
                        action,
                        &mut state,
                    );
                    needs_redraw |= result.dirty;
                }
            }
        }

        // Watchdog: the select should only return on a real event, a timed wake,
        // or an async action. If it ever returns with nothing scheduled, it is
        // spinning (a select branch became immediately ready) — which would burn
        // 100% CPU and starve keyboard input. Force a short sleep to break the
        // spin and let a real event (key/mouse) be picked up.
        if !needs_redraw && !timed_redraw && !state.should_quit {
            mouse_interaction.idle_iterations += 1;
            if mouse_interaction.idle_iterations >= MAX_IDLE_ITERATIONS {
                tracing::warn!("event loop spin detected; forcing a sleep to break it");
                tokio::time::sleep(Duration::from_millis(8)).await;
                mouse_interaction.idle_iterations = 0;
            }
        } else {
            mouse_interaction.idle_iterations = 0;
        }

        if state.should_quit {
            break;
        }
    }

    // Persist the session before tearing the terminal down so a relaunch
    // restores the open tabs. Best-effort: a write failure is logged, not fatal.
    if let Err(e) = crate::app::session::persist_session(&state) {
        tracing::warn!("failed to save TUI session: {e}");
    }

    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
        crossterm::event::DisableFocusChange,
        crossterm::event::DisableBracketedPaste
    )?;
    Ok(())
}

/// Restores the terminal on drop so raw mode / alternate screen / mouse capture
/// are always cleaned up, even if the event loop exits via an error (`?`) or a
/// panic. Without this, an early return leaves the terminal in raw mode and the
/// alternate screen, which makes it look frozen and unresponsive to keys.
/// Restore the terminal to its normal state: raw mode off, alternate screen
/// left, mouse capture off, hardware cursor back to the user's default shape.
///
/// Every step ignores its error and the whole function is idempotent, so it is
/// safe to call from a panic hook: a partially restored terminal (or a second
/// call after the normal teardown) must never block the rest of the cleanup or
/// make the crash report itself fail.
pub fn restore_terminal() {
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
        crossterm::event::DisableFocusChange,
        crossterm::event::DisableBracketedPaste
    );
    let _ = crate::common::editor::reset_hardware_cursor();
    let _ = crossterm::execute!(std::io::stdout(), crossterm::cursor::Show);
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

/// The next instant at which a timed repaint is due, if any. Returns `None`
/// when there is no periodic work pending, so the loop sleeps until a real
/// event arrives (~0% CPU idle), matching the original dbm's "Route B".
fn next_wake(state: &AppState) -> Option<Instant> {
    let now = Instant::now();
    // Keep repainting while a discover scan's progress is advancing.
    let scanning = state.discover.scanning;
    // Wake shortly after the FPS/waste counters would go stale so they decay
    // to 0 on idle (only while they are live/non-zero). Keyed off the *real*
    // frame time so forced counter-refresh repaints cannot keep this alive.
    let counters_live = state
        .perf
        .last_real_frame_elapsed()
        .is_some_and(|l| l <= Duration::from_millis(250))
        && (state.perf.fps > 0.0 || state.perf.redundancy_rate > 0.0);
    if scanning || counters_live {
        Some(now + TICK_RATE)
    } else {
        None
    }
}

/// Sleep until `deadline`, or forever when there is none (so an idle loop has
/// no timer and burns no CPU). Only *future* deadlines are ever passed in by
/// [`next_wake`]; a past deadline would make this return instantly and spin.
async fn sleep_until_opt(deadline: Option<Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(tokio::time::Instant::from_std(d)).await,
        None => std::future::pending::<()>().await,
    }
}

/// The rect of the footer's self-updating fps/waste slot, excluded from the
/// changed-cell count so it doesn't mask real redundancy. The perf readout is
/// the rightmost `perf_w` columns of the bottom `footer_h` rows.
fn perf_exclude_rects(size: ratatui::layout::Size, footer_h: u16) -> Vec<Rect> {
    if footer_h == 0 || size.height < footer_h {
        return Vec::new();
    }
    let perf_w = 23u16.min(size.width);
    let x = size.width.saturating_sub(perf_w);
    vec![Rect::new(
        x,
        size.height - footer_h,
        size.width - x,
        footer_h,
    )]
}

/// A round triggered by an external message (keyboard/tick). The seed message
/// and any already-available async actions are queued and drained iteratively.
/// Returns the aggregated [`UpdateResult`] so the caller can decide whether the
/// round actually changed rendering state (`dirty`).
pub(crate) fn process_message_round(
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    seed: AppMsg,
    state: &mut AppState,
) -> UpdateResult {
    let mut pending: VecDeque<AppMsg> = VecDeque::new();
    pending.push_back(seed);
    drain_async_actions(action_rx, &mut pending);
    drain_pending(effect_runner, &mut pending, state)
}

/// A round triggered by an async action (an effect result). The action is
/// applied via `handle_action`; its intents/effects are consumed by the same
/// iterative drain as a message round.
fn process_action_round(
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    seed: Action,
    state: &mut AppState,
) -> UpdateResult {
    let mut pending: VecDeque<AppMsg> = VecDeque::new();
    let result = handle_action(seed, state);
    // The seed's own dirty flag must survive into the returned result, or a
    // completion/error that only mutates state (no queued cascade) would be
    // applied without ever scheduling a repaint — leaving a stale frame (e.g.
    // a "scanning… N/N" footer) on screen after the scan already finished.
    let mut aggregated = UpdateResult::new();
    aggregated.dirty |= result.dirty;
    queue_result(effect_runner, result, &mut pending);
    drain_async_actions(action_rx, &mut pending);
    aggregated.dirty |= drain_pending(effect_runner, &mut pending, state).dirty;
    aggregated
}

/// Iteratively apply every queued message until the queue is empty or the
/// per-round budget is exhausted. Intents produced by an update resolve to new
/// `AppMsg`s that are pushed back onto the queue, replacing recursion with a
/// heap-backed work list.
///
/// Returns an aggregated [`UpdateResult`] whose `dirty` is the OR of every
/// applied update: if *any* message in the round changed rendering state, the
/// round as a whole is considered dirty so the caller repaints once.
fn drain_pending(
    effect_runner: &EffectRunner<Action>,
    pending: &mut VecDeque<AppMsg>,
    state: &mut AppState,
) -> UpdateResult {
    let mut processed = 0usize;
    let mut aggregated = UpdateResult::new();
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

        // Use `update_unchecked`: keyboard focus routing is done in the input
        // layer (`key_to_msg` only produces messages for the active pane), so
        // the focus guard is redundant for keyboard input and actively harmful
        // for programmatic messages — an intent or pending cascade may target a
        // *non-focused* pane (e.g. updating the explorer tree while the focus
        // sits on the workspace). Such cross-pane updates must not be dropped.
        let result = update_unchecked(msg, state);
        aggregated.dirty |= result.dirty;
        queue_result(effect_runner, result, pending);
    }
    aggregated
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
        // Cross-feature intents (routed by a parent) produce no message and are
        // skipped here rather than panicking.
        if let Some(nested) = IntentRouter::route::<AppMsg>(intent) {
            pending.push_back(nested);
        }
    }
    pending.extend(result.pending);
}

/// Convert an effect-produced action into the message(s) it should dispatch
/// back into the router.
///
/// This is the single conversion used by both the message-round drain and the
/// action-round seed (`handle_action`), so an async action is handled
/// identically no matter how it arrives — eliminating the previous race where
/// a feature action received as a `recv()` seed was dropped by `handle_action`
/// and never drained.
pub(crate) fn action_to_app_msgs(action: Action) -> Vec<AppMsg> {
    match action {
        Action::Dispatch(msg) => vec![msg],
        Action::Shell(crate::app_shell::action::ShellAction::Quit) => {
            vec![AppMsg::Shell(crate::app_shell::msg::ShellMsg::Quit)]
        }
        // The discover feature's scan/register actions feed back into the
        // discover modal as messages.
        Action::Discover(action) => vec![AppMsg::Discover(
            crate::features::discover::msg::DiscoverMsg::Message(discover_action_to_msg(action)),
        )],
        // The explorer's load actions feed back into the explorer.
        Action::Explorer(action) => vec![AppMsg::Explorer(
            crate::features::explorer::msg::ExplorerMsg::Message(explorer_action_to_msg(action)),
        )],
        // The instance workspace's load/save/delete actions feed back.
        Action::Iw(action) => vec![AppMsg::Iw(
            crate::features::instance_workspace::msg::IwMsg::Message(iw_action_to_msg(action)),
        )],
        // The SQL workspace's catalog-load actions feed back into the targeted
        // tab's editor (the context picker). A commit completion also closes
        // the commit-preview modal.
        Action::Sql(action) => {
            use crate::features::sql_workspace::effect::SqlAction;
            use crate::features::sql_workspace::sql_tab::effect::SqlTabAction;
            use crate::features::sql_workspace::sql_tab::results::effect::ResultsAction;
            let mut msgs = Vec::new();
            match &action {
                SqlAction::SqlTab(SqlTabAction::Results {
                    action: ResultsAction::CommitResult { .. },
                    ..
                }) => {
                    msgs.push(AppMsg::CloseModal);
                }
                // A failed query surfaces its message in the results pane (via
                // the routed `QueryError` message) and in the global footer
                // status line, mirroring the original dbm's `set_status`.
                SqlAction::SqlTab(SqlTabAction::Results {
                    action: ResultsAction::QueryError { message },
                    ..
                }) => {
                    msgs.push(AppMsg::Footer(
                        crate::features::global_footer::msg::FooterMsg::Message(
                            crate::features::global_footer::msg::FooterMessage::SetStatus(format!(
                                "Query failed: {message}"
                            )),
                        ),
                    ));
                }
                _ => {}
            }
            msgs.push(AppMsg::Sql(
                crate::features::sql_workspace::msg::SqlMsg::Message(sql_action_to_msg(action)),
            ));
            msgs
        }
        // Header/footer/perf effects are currently stateless; nothing to route.
        Action::Header(_) | Action::Footer(_) | Action::Perf(_) => Vec::new(),
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
            Ok(action) => pending.extend(action_to_app_msgs(action)),
            Err(mpsc::error::TryRecvError::Empty) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => break,
        }
    }
}

/// Convert a discover action into the corresponding discover message.
fn discover_action_to_msg(
    action: crate::features::discover::effect::DiscoverAction,
) -> crate::features::discover::msg::DiscoverMessage {
    use crate::features::discover::effect::DiscoverAction as A;
    use crate::features::discover::msg::DiscoverMessage as M;
    match action {
        A::ScanProgress { done, total } => M::ScanProgress { done, total },
        A::ScanComplete { items } => M::ScanComplete { items },
        A::ScanCancelled => M::ScanCancelled,
        A::ScanError { error } => M::ScanError { error },
        A::RegisterComplete { items } => M::RegisterComplete { items },
        A::RegisterError { error } => M::RegisterError { error },
    }
}

/// Convert an instance workspace action into the corresponding iw message.
fn iw_action_to_msg(
    action: crate::features::instance_workspace::effect::IwAction,
) -> crate::features::instance_workspace::msg::IwMessage {
    use crate::features::instance_workspace::connections::effect::ConnectionsAction as CA;
    use crate::features::instance_workspace::connections::msg::{
        ConnectionsMessage, ConnectionsMsg,
    };
    use crate::features::instance_workspace::effect::IwAction as IA;
    use crate::features::instance_workspace::msg::IwMessage as IM;
    use crate::features::instance_workspace::overview::effect::OverviewAction as OA;
    use crate::features::instance_workspace::overview::msg::{OverviewMessage, OverviewMsg};
    match action {
        IA::Overview(action) => match action {
            OA::Loaded { instance } => {
                IM::Overview(OverviewMsg::Message(OverviewMessage::Loaded { instance }))
            }
            OA::Error { error } => {
                tracing::warn!("iw overview load failed: {error}");
                IM::Overview(OverviewMsg::Message(OverviewMessage::Reload))
            }
        },
        IA::Connections(action) => match action {
            CA::Loaded { connections } => {
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::Loaded {
                    connections,
                }))
            }
            CA::Saved => {
                // The form is still open until the save succeeds; `Saved` closes
                // it and reloads the list (a failed save keeps the form via
                // `SaveError`, so it is not silently dropped).
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::Saved))
            }
            CA::Deleted => {
                // Reload the connections after a delete (no form involved).
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::Reload))
            }
            CA::Error { error } => {
                tracing::warn!("iw connections op failed: {error}");
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::SaveError(
                    error,
                )))
            }
            CA::TestResult { ok, error } => {
                // Match the original dbm's wording and color: "Test OK" in green
                // on success, "Test failed: <reason>" in red on failure, with a
                // timestamp prefix to limit waste on repeat tests.
                let ts = crate::common::utils::time::utc_timestamp();
                let (status, kind) = if ok {
                    (
                        format!("{ts} Test OK"),
                        crate::features::instance_workspace::connections::state::ConnectionStatusKind::Success,
                    )
                } else {
                    (
                        format!(
                            "{ts} Test failed: {}",
                            error.unwrap_or_else(|| "could not connect".to_string())
                        ),
                        crate::features::instance_workspace::connections::state::ConnectionStatusKind::Failure,
                    )
                };
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::SetStatus {
                    status,
                    kind,
                }))
            }
            CA::ListTestResult { ok, error } => {
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::TestComplete {
                    ok,
                    error,
                }))
            }
        },
        IA::Unregistered { instance } => IM::Unregistered { instance },
    }
}

/// Convert a SQL workspace action into the corresponding workspace message,
/// routing editor actions back to the originating tab's editor (the context
/// picker's catalog results).
fn sql_action_to_msg(
    action: crate::features::sql_workspace::effect::SqlAction,
) -> crate::features::sql_workspace::msg::SqlMessage {
    use crate::features::sql_workspace::effect::SqlAction as SA;
    use crate::features::sql_workspace::msg::SqlMessage;
    use crate::features::sql_workspace::sql_tab::editor::context_picker::msg::ContextPickerMsg;
    use crate::features::sql_workspace::sql_tab::editor::effect::EditorAction as EA;
    use crate::features::sql_workspace::sql_tab::editor::msg::{EditorMessage, EditorMsg};
    use crate::features::sql_workspace::sql_tab::effect::SqlTabAction as STA;
    use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
    match action {
        SA::SqlTab(STA::Editor { tab_id, action }) => {
            let msg = match action {
                EA::ContextPicker(cp) => EditorMsg::Message(EditorMessage::ContextPicker(
                    ContextPickerMsg::Message(cp_action_to_msg(cp)),
                )),
                EA::CompletionCatalogLoaded(data) => {
                    EditorMsg::Message(EditorMessage::CatalogLoaded {
                        tables: data.tables,
                        columns_by_table: data.columns_by_table,
                    })
                }
            };
            SqlMessage::SqlTab(SqlTabMsg::Message(SqlTabMessage::Editor { tab_id, msg }))
        }
        SA::SqlTab(STA::Results { tab_id, action }) => {
            let results_msg = results_action_to_msg(action);
            SqlMessage::SqlTab(SqlTabMsg::Message(SqlTabMessage::Results {
                tab_id,
                msg: crate::features::sql_workspace::sql_tab::results::msg::ResultsMsg::Message(
                    results_msg,
                ),
            }))
        }
        SA::SqlTab(STA::History { tab_id, action }) => {
            use crate::features::sql_workspace::sql_tab::history::effect::HistoryAction as HA;
            use crate::features::sql_workspace::sql_tab::history::store::SqlHistoryStore;
            match action {
                HA::HistoryLoaded {
                    instance,
                    connection,
                    entries,
                } => {
                    let mut store = SqlHistoryStore::default();
                    if !entries.is_empty() {
                        let mut map = std::collections::HashMap::new();
                        map.insert((instance, connection), entries);
                        store = SqlHistoryStore::from_map(map);
                    }
                    SqlMessage::SqlTab(SqlTabMsg::Message(SqlTabMessage::SetHistoryStore {
                        tab_id,
                        store,
                    }))
                }
            }
        }
    }
}

/// Convert a results action into the corresponding results message.
fn results_action_to_msg(
    action: crate::features::sql_workspace::sql_tab::results::effect::ResultsAction,
) -> crate::features::sql_workspace::sql_tab::results::msg::ResultsMessage {
    use crate::features::sql_workspace::sql_tab::results::effect::ResultsAction as RA;
    use crate::features::sql_workspace::sql_tab::results::msg::ResultsMessage as M;
    match action {
        RA::ResultReady { result, paginated } => M::SetResult { result, paginated },
        RA::QueryError { message } => {
            tracing::warn!("query failed: {message}");
            // Mirror the original dbm: clear the result and surface the error
            // text in the results pane (stored as `query_error`).
            M::QueryError { message }
        }
        RA::CommitResult { ok, message } => {
            tracing::info!("commit ok={ok}: {message}");
            M::ResetSelection
        }
        RA::EditabilityReady { target, blocked } => M::EditabilityReady { target, blocked },
    }
}

/// Convert a context picker action into the corresponding picker message.
fn cp_action_to_msg(
    action: crate::features::sql_workspace::sql_tab::editor::context_picker::effect::ContextPickerAction,
) -> crate::features::sql_workspace::sql_tab::editor::context_picker::msg::ContextPickerMessage {
    use crate::features::sql_workspace::sql_tab::editor::context_picker::effect::ContextPickerAction as CPA;
    use crate::features::sql_workspace::sql_tab::editor::context_picker::msg::ContextPickerMessage as M;
    match action {
        CPA::DatabasesLoaded { items } => M::DatabasesLoaded { items },
        CPA::DatabasesError { error } => M::DatabasesError { error },
        CPA::SchemasLoaded { items } => M::SchemasLoaded { items },
        CPA::SchemasError { error } => M::SchemasError { error },
    }
}

/// Convert an explorer action into the corresponding explorer message.
fn explorer_action_to_msg(
    action: crate::features::explorer::effect::ExplorerAction,
) -> crate::features::explorer::msg::ExplorerMessage {
    use crate::features::explorer::effect::ExplorerAction as EA;
    use crate::features::explorer::instances::effect::InstancesAction as IA;
    use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
    use crate::features::explorer::msg::ExplorerMessage as EM;
    match action {
        EA::Instances(action) => match action {
            IA::InstancesLoaded { instances } => {
                EM::Instances(InstancesMsg::Message(InstancesMessage::Loaded {
                    instances,
                }))
            }
            IA::ConnectionsLoaded {
                instance_idx,
                connections,
            } => EM::Instances(InstancesMsg::Message(InstancesMessage::ConnectionsLoaded {
                instance_idx,
                connections,
            })),
            IA::LoadError { error } => {
                tracing::warn!("explorer load failed: {error}");
                EM::Instances(InstancesMsg::Message(InstancesMessage::Load))
            }
        },
        EA::Objects(action) => EM::Objects(objects_action_to_msg(action)),
    }
}

/// Convert an objects action into the corresponding objects message.
fn objects_action_to_msg(
    action: crate::features::explorer::objects::effect::ObjectsAction,
) -> crate::features::explorer::objects::msg::ObjectsMsg {
    use crate::features::explorer::objects::effect::ObjectsAction as OA;
    use crate::features::explorer::objects::msg::ObjectsMessage as M;
    let msg = match action {
        OA::DatabasesLoaded { databases } => M::DatabasesLoaded { databases },
        OA::DatabasesError { error } => {
            tracing::warn!("objects database load failed: {error}");
            M::DatabasesError { error }
        }
        OA::SchemasLoaded { database, schemas } => M::SchemasLoaded { database, schemas },
        OA::SchemasError { database, error } => {
            tracing::warn!("objects schemas load failed for {database}: {error}");
            M::SchemasError { database, error }
        }
        OA::ExtensionsLoaded {
            database,
            extensions,
        } => M::ExtensionsLoaded {
            database,
            extensions,
        },
        OA::ExtensionsError { database, error } => {
            tracing::warn!("objects extensions load failed for {database}: {error}");
            M::ExtensionsError { database, error }
        }
        OA::ObjectListLoaded {
            database,
            schema,
            kind,
            items,
        } => M::ObjectListLoaded {
            database,
            schema,
            kind,
            items,
        },
        OA::ObjectListError {
            database,
            schema,
            kind,
            error,
        } => {
            tracing::warn!("objects {kind:?} load failed for {database}.{schema}: {error}");
            M::ObjectListError {
                database,
                schema,
                kind,
                error,
            }
        }
    };
    crate::features::explorer::objects::msg::ObjectsMsg::Message(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_shell::pane::Pane;
    use crate::features::sql_workspace::effect::SqlAction;
    use crate::features::sql_workspace::msg::SqlMessage;
    use crate::features::sql_workspace::sql_tab::effect::SqlTabAction;
    use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
    use crate::features::sql_workspace::sql_tab::results::effect::ResultsAction;
    use crate::features::sql_workspace::sql_tab::results::msg::{ResultsMessage, ResultsMsg};
    use crate::features::sql_workspace::sql_tab::results::state::QueryResultData;

    #[test]
    fn sql_results_result_ready_routes_to_set_result() {
        let action = SqlAction::SqlTab(SqlTabAction::Results {
            tab_id: 3,
            action: ResultsAction::ResultReady {
                result: QueryResultData {
                    columns: vec![],
                    rows: vec![vec!["1".into()]],
                    rows_affected: None,
                    total_rows: Some(1),
                },
                paginated: true,
            },
        });
        let SqlMessage::SqlTab(SqlTabMsg::Message(SqlTabMessage::Results { tab_id, msg })) =
            sql_action_to_msg(action)
        else {
            panic!("expected Results route");
        };
        assert_eq!(tab_id, 3);
        let ResultsMsg::Message(ResultsMessage::SetResult { result, paginated }) = msg else {
            panic!("expected SetResult");
        };
        assert!(paginated);
        assert_eq!(result.rows[0][0], "1");
    }

    #[test]
    fn sql_results_query_error_maps_to_query_error_message() {
        let action = SqlAction::SqlTab(SqlTabAction::Results {
            tab_id: 1,
            action: ResultsAction::QueryError {
                message: "boom".into(),
            },
        });
        let SqlMessage::SqlTab(SqlTabMsg::Message(SqlTabMessage::Results { tab_id, msg })) =
            sql_action_to_msg(action)
        else {
            panic!("expected Results route");
        };
        assert_eq!(tab_id, 1);
        assert!(matches!(
            msg,
            ResultsMsg::Message(ResultsMessage::QueryError { message }) if message == "boom"
        ));
    }

    #[test]
    fn sql_history_loaded_routes_to_set_history_store() {
        use crate::features::sql_workspace::sql_tab::history::effect::HistoryAction;
        let action = SqlAction::SqlTab(SqlTabAction::History {
            tab_id: 7,
            action: HistoryAction::HistoryLoaded {
                instance: "inst".into(),
                connection: "c1".into(),
                entries: vec!["SELECT 1".into(), "SELECT 2".into()],
            },
        });
        let SqlMessage::SqlTab(SqlTabMsg::Message(SqlTabMessage::SetHistoryStore {
            tab_id,
            store,
        })) = sql_action_to_msg(action)
        else {
            panic!("expected SetHistoryStore route");
        };
        assert_eq!(tab_id, 7);
        assert_eq!(store.entries("inst", "c1"), &["SELECT 1", "SELECT 2"]);
    }

    #[test]
    fn perf_exclude_rects_covers_the_footer_stats_slot() {
        // A 100x30 terminal with a 1-row footer excludes the rightmost 23 cols.
        let rects = perf_exclude_rects(ratatui::layout::Size::new(100, 30), 1);
        assert_eq!(rects.len(), 1);
        let r = rects[0];
        assert_eq!(r.x, 100 - 23);
        assert_eq!(r.width, 23);
        assert_eq!(r.y, 30 - 1);
        assert_eq!(r.height, 1);
        // A multi-row footer excludes the whole right strip of the footer.
        let rects = perf_exclude_rects(ratatui::layout::Size::new(100, 30), 2);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].height, 2);
        // A terminal narrower than the perf slot excludes the entire width.
        let rects = perf_exclude_rects(ratatui::layout::Size::new(10, 30), 1);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].x, 0);
        assert_eq!(rects[0].width, 10);
        // No footer -> nothing to exclude.
        assert!(perf_exclude_rects(ratatui::layout::Size::new(100, 30), 0).is_empty());
    }

    #[test]
    fn next_wake_sleeps_forever_when_idle() {
        let state = crate::app::state::AppState::default();
        // Idle (no scan, counters zero / never drawn) -> no timed wake.
        assert_eq!(next_wake(&state), None);
        // Scanning forces a periodic wake.
        let mut state = crate::app::state::AppState::default();
        state.discover.scanning = true;
        assert!(next_wake(&state).is_some());
    }

    #[test]
    fn action_round_propagates_seed_dirty_flag() {
        // Regression: a completion action that only mutates state (no queued
        // cascade) must still mark the round dirty, otherwise the event loop
        // skips the repaint and leaves a stale "scanning…" frame on screen.
        let (action_tx, mut action_rx) = mpsc::unbounded_channel::<Action>();
        let services = std::sync::Arc::new(crate::common::service::services::Services::default());
        let (effect_runner, handle) = EffectRunner::new(action_tx, services);
        drop(handle); // no effects run in this test; the runner is only a sink

        let mut state = crate::app::state::AppState::default();
        state.discover.scanning = true;
        state.discover.scan_progress = Some((10, 11));

        let action = crate::app::action::Action::Discover(
            crate::features::discover::effect::DiscoverAction::ScanComplete { items: vec![] },
        );
        let result = process_action_round(&effect_runner, &mut action_rx, action, &mut state);

        assert!(result.dirty, "completion must mark the round dirty");
        assert!(
            !state.discover.scanning,
            "scanning must clear on completion"
        );
        assert_eq!(state.discover.scan_progress, None);
    }

    #[test]
    fn full_app_render_does_not_panic_at_extreme_history_split_widths() {
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        // Render the whole app (workspace border, footer, sql tab, history) at
        // the extreme detail/history split widths reachable by dragging the
        // internal detail/list splitter. A panic here would leave the terminal
        // in raw/alternate-screen mode and make it look "unresponsive".
        for detail_w in 24u16..=72 {
            for history_w in [12u16, 24, 46, 84, 100, 200] {
                let mut state = AppState::default();
                state.focus = Pane::SQLWorkspace;
                state.sql.sql_tab.open_connection_tab(
                    "inst".into(),
                    "c1".into(),
                    "id1".into(),
                    None,
                    None,
                    None,
                );
                state
                    .sql
                    .sql_tab
                    .history_store
                    .record_success("inst", "c1", "SELECT * FROM users");
                let tab = &mut state.sql.sql_tab.tabs[0];
                tab.focus = SqlFocus::History;
                tab.splitter.history_pane_width = history_w;
                tab.history.splitter.detail_pane_width = detail_w;
                // Give the editor real SQL content so it is exercised at the
                // narrow width the widened history zone leaves it.
                tab.editor =
                    crate::features::sql_workspace::sql_tab::editor::state::EditorState::with_sql(
                        "SELECT * FROM \"测试表\" WHERE id = 1 AND name ILIKE '%foo%' ORDER BY created_at DESC",
                    );
                // Wide terminal to match the real app (which can be > 100 cols).
                let mut terminal = Terminal::new(TestBackend::new(160, 50)).unwrap();
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    terminal
                        .draw(|frame| {
                            let _ = crate::app::view::render(frame, &state);
                        })
                        .unwrap();
                }));
                assert!(
                    r.is_ok(),
                    "full-app render panicked at detail_w={detail_w}, history_w={history_w}"
                );
            }
        }
    }
}
