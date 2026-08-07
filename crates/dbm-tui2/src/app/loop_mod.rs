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
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::app::action::Action;
use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app::update::{handle_action, update_unchecked, UpdateResult};
use crate::app::view::render;
use crate::app_shell::effect::EffectRunner;
use crate::app_shell::intent::IntentRouter;
use crate::app_shell::pane::Pane;
use crate::features::global_footer::view as footer_view;
use crate::features::header::msg::{HeaderMessage, HeaderMsg};
use crate::features::perf_monitor::backend::CountingBackend;

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
        crossterm::event::EnableBracketedPaste
    )?;
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
    // Restore a previously persisted session (open tabs + focus) before the
    // first frame so the shell comes up where the user left it.
    if let Err(e) = crate::app::session::restore_session(&mut state) {
        tracing::warn!("failed to restore TUI session: {e}");
    }
    let mut reader = EventStream::new();

    // Populate the explorer tree on startup. Use `update_unchecked` so the
    // message is not dropped by the focus guard: at startup focus is still the
    // header (or the restored session pane), so a guarded `Explorer(Load)`
    // would be discarded and the tree would stay empty until the explorer
    // gained focus.
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
    // populated *before* the first frame renders. Otherwise the tree shows
    // empty on the first paint and only appears after a later event triggers a
    // repaint (a start-of-session flicker).
    if let Some(action) = action_rx.recv().await {
        process_action_round(&effect_runner, &mut action_rx, action, &mut state);
    }

    // Which SQL-tab splitter is being drag-resized, if any. This is transient
    // interaction state that lives only for the lifetime of a drag gesture; it
    // never reaches `AppState` (TEA: state mutations still flow through
    // `update` via split-resize messages).
    let mut split_drag: Option<crate::features::sql_workspace::sql_tab::layout::SqlSplitter> = None;

    // Event-driven, on-demand redraw (mirrors the original dbm "Route B"): the
    // screen is only repainted when a real event/action changed state, or when
    // a timed refresh is actually due. With no timed work pending the app
    // sleeps until a real event arrives (~0% CPU).
    let mut needs_redraw = true;
    // A repaint requested by a timed/forced source (counter decay, discover
    // scan tick) rather than a real event. Such repaints must NOT feed the
    // FPS/waste estimates, so they are tracked separately from `needs_redraw`.
    let mut timed_redraw = false;

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
            let footer_h = footer_view::footer_height(&state.footer, size.width);
            terminal.backend_mut().set_exclude_rects(perf_exclude_rects(size, footer_h));
            // Draw the current frame. The perf_monitor feature is passive: the
            // run loop samples each frame here and feeds the smoothed FPS and
            // redundant-redraw ratio from the wrapped backend.
            terminal.draw(|frame| render(frame, &state))?;
            let changed_cells = terminal.backend_mut().last_changed_cells();
            if real_redraw {
                // Debug assertion (non-fatal): a real redraw (one asked for by
                // an event/action, i.e. dirty) that changed zero cells means the
                // repaint was over-broad — a message marked `dirty` without
                // actually changing rendered state. Timed/forced repaints never
                // reach this branch, so this only surfaces the event-driven
                // dirty case.
                if changed_cells == 0 {
                    tracing::debug!("dirty redraw changed 0 cells (over-broad dirty?)");
                }
                state.perf.record_frame();
                state.perf.record_redundancy(changed_cells);
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
                        // `q` or `CTRL+D` quits through the standard message
                        // flow so the handler sets the quit flag.
                        (_, KeyCode::Char('q')) | (true, KeyCode::Char('d')) => {
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
                    use crossterm::event::{MouseButton, MouseEventKind};
                    use ratatui::prelude::Position;
                    tracing::trace!(
                        kind = ?mouse.kind,
                        col = mouse.column,
                        row = mouse.row,
                        "mouse event received"
                    );
                    let point = Position::new(mouse.column, mouse.row);
                    // Aggregated across the dispatched messages below: the round
                    // repaints only if one of them changed rendered state.
                    let mut dirty = false;
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) if state.modal.is_none() => {
                            // Map the click to a focus zone by region. The layout
                            // mirrors `app/view.rs`: header (top 3 rows), explorer
                            // (left 20% of the body), workspace (right 80%).
                            let size = terminal.size()?;
                            let footer_h = footer_view::footer_height(&state.footer, size.width);
                            let body_top = 3u16;
                            let body_h =
                                size.height.saturating_sub(body_top).saturating_sub(footer_h);
                            let explorer_w = (size.width.saturating_mul(2) / 10).max(1);

                            // While the discover parent pane is open, clicking
                            // inside its popup switches the active discover child
                            // sub-pane (engine / targets / results), mirroring
                            // Ctrl+j/k. Clicks outside the popup keep discover
                            // focused (the discover overlay owns input).
                            let target_pane = if !matches!(state.focus, Pane::Discover(_)) {
                                if mouse.row < body_top {
                                    Some(Pane::Header)
                                } else if mouse.row >= body_top + body_h {
                                    None
                                } else if mouse.column < explorer_w {
                                    Some(Pane::Explorer(
                                        crate::app_shell::nav::ExplorerPane::default(),
                                    ))
                                } else if state.iw.instance_name.is_empty() {
                                    Some(Pane::Workspace)
                                } else {
                                    Some(Pane::InstanceWorkspace)
                                }
                            } else {
                                None
                            };
                            tracing::debug!(
                                point = ?point,
                                target_pane = ?target_pane,
                                current_focus = ?state.focus,
                                "mouse click pane mapping"
                            );
                            if let Some(pane) = target_pane.filter(|z| *z != state.focus) {
                                let msg = AppMsg::Shell(
                                    crate::app_shell::msg::ShellMsg::FocusChanged { pane },
                                );
                                let result = process_message_round(
                                    &effect_runner,
                                    &mut action_rx,
                                    msg,
                                    &mut state,
                                );
                                dirty |= result.dirty;
                            }

                            // Inside the discover popup: map the click's row to a
                            // discover child pane and switch focus to it.
                            if let Pane::Discover(sub) = state.focus
                                && let Some(next) = discover_subpane_for_click(
                                    mouse.column,
                                    mouse.row,
                                    explorer_w,
                                    body_top,
                                    body_h,
                                    size.width,
                                )
                                && next != sub
                            {
                                let msg = AppMsg::Discover(
                                    crate::features::discover::msg::DiscoverMsg::Message(
                                        crate::features::discover::msg::DiscoverMessage::Focus(next),
                                    ),
                                );
                                let result = process_message_round(
                                    &effect_runner,
                                    &mut action_rx,
                                    msg,
                                    &mut state,
                                );
                                dirty |= result.dirty;
                                tracing::debug!(to = ?next, "mouse click switched discover sub-pane");
                            }

                            // Left-click on the header `Discover` button activates
                            // it, in addition to moving focus to the header.
                            let header_area = Rect::new(0, 0, size.width, 3);
                            let button_rect =
                                crate::features::header::view::discover_button_rect(header_area);
                            let clicked = button_rect.is_some_and(|r| r.contains(point));
                            tracing::debug!(clicked, "header button click resolved");
                            if clicked {
                                // Clicking the header button is an explicit user
                                // intent: move focus to the Header zone (via the
                                // shell message), then dispatch Activate. Both go
                                // through `update` so every state change flows
                                // through the single state-transition channel.
                                let result = process_message_round(
                                    &effect_runner,
                                    &mut action_rx,
                                    AppMsg::Shell(crate::app_shell::msg::ShellMsg::FocusChanged {
                                        pane: Pane::Header,
                                    }),
                                    &mut state,
                                );
                                dirty |= result.dirty;
                                let result = process_message_round(
                                    &effect_runner,
                                    &mut action_rx,
                                    AppMsg::Header(HeaderMsg::Message(HeaderMessage::Activate)),
                                    &mut state,
                                );
                                dirty |= result.dirty;
                                tracing::debug!("dispatching HeaderMessage::Activate");
                            }

                            // Starting a drag on a SQL-tab splitter begins a
                            // resize gesture (only when the SQL workspace owns
                            // focus and it is actually rendered).
                            if state.focus == Pane::Workspace
                                && let Some((layout, _tab_id)) =
                                    sql_tab_layout_for_hit(terminal.size()?, &state)
                                && let Some(splitter) = layout.splitter_at(point.x, point.y)
                            {
                                split_drag = Some(splitter);
                                tracing::debug!(?splitter, "splitter drag started");
                            }
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if let Some(splitter) = split_drag {
                                let size = terminal.size()?;
                                if let Some((layout, tab_id)) = sql_tab_layout_for_hit(size, &state) {
                                    // Compute the new split value from the mouse
                                    // position and dispatch a resize message (all
                                    // state changes flow through `update`).
                                    let msg = split_resize_msg(splitter, point, layout, tab_id);
                                    let result = process_message_round(
                                        &effect_runner,
                                        &mut action_rx,
                                        msg,
                                        &mut state,
                                    );
                                    dirty |= result.dirty;
                                }
                            }
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            if split_drag.take().is_some() {
                                tracing::debug!("splitter drag finished");
                            }
                        }
                        _ => {
                            tracing::trace!(
                                is_left_down = matches!(
                                    mouse.kind,
                                    MouseEventKind::Down(MouseButton::Left)
                                ),
                                modal_open = state.modal.is_some(),
                                "mouse event ignored"
                            );
                        }
                    }
                    // Repaint only if a dispatched message actually changed
                    // rendered state (e.g. moved focus, activated the header
                    // button, or resized a splitter). A click that changed
                    // nothing, or an ignored event, skips the redraw.
                    needs_redraw |= dirty;
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
        crossterm::event::DisableBracketedPaste
    )?;
    Ok(())
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
    vec![Rect::new(x, size.height - footer_h, size.width - x, footer_h)]
}

/// Compute the active SQL tab's body layout for mouse hit-testing, using the
/// tab's stored split values. Returns the layout and the tab's session id, or
/// `None` when the SQL workspace is not currently rendered (a modal is open,
/// the instance workspace is showing, or no tab is active).
fn sql_tab_layout_for_hit(
    size: ratatui::layout::Size,
    state: &AppState,
) -> Option<(crate::features::sql_workspace::sql_tab::layout::SqlTabLayout, usize)> {
    use crate::features::sql_workspace::sql_tab::layout::sql_tab_layout;

    if state.modal.is_some()
        || matches!(state.focus, Pane::Discover(_))
        || !state.iw.instance_name.is_empty()
    {
        return None;
    }
    let tab = state.sql.sql_tab.tabs.get(state.sql.sql_tab.active_tab)?;
    let footer_h = footer_view::footer_height(&state.footer, size.width);
    let body_top = 3u16;
    let body_h = size.height.saturating_sub(body_top).saturating_sub(footer_h);
    if body_h < 3 {
        return None;
    }
    let explorer_w = (size.width.saturating_mul(2) / 10).max(1);
    let workspace_w = size.width.saturating_sub(explorer_w);
    let workspace = Rect::new(explorer_w, body_top, workspace_w, body_h);
    // The SQL tab's body sits below its 1-row tab bar within the workspace.
    let sql_body = Rect::new(
        workspace.x,
        workspace.y + 1,
        workspace.width,
        workspace.height.saturating_sub(1),
    );
    let layout = sql_tab_layout(sql_body, tab.split_ratio, tab.history_pane_width);
    if layout.editor.width == 0 {
        return None;
    }
    Some((layout, tab.session.id))
}

/// Build the split-resize message for a drag gesture at `point`, using the
/// current layout as the reference frame.
fn split_resize_msg(
    splitter: crate::features::sql_workspace::sql_tab::layout::SqlSplitter,
    point: ratatui::prelude::Position,
    layout: crate::features::sql_workspace::sql_tab::layout::SqlTabLayout,
    tab_id: usize,
) -> AppMsg {
    use crate::features::sql_workspace::sql_tab::layout::SqlSplitter;
    use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
    use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};

    let msg = match splitter {
        SqlSplitter::EditorResults => {
            // The splitter's row becomes the top-pane height; express it as a
            // percent of the body height so the ratio survives terminal resizes.
            let body_top = layout.editor.y;
            let body_h = layout.results.bottom().saturating_sub(body_top).max(1);
            let top_h = point.y.saturating_sub(body_top);
            let ratio = ((u32::from(top_h) * 100) / u32::from(body_h)).min(99) as u8;
            SqlTabMessage::SetSplitRatio { tab_id, ratio }
        }
        SqlSplitter::EditorHistory => {
            // The history pane owns the right side; its width is the distance
            // from the splitter to the right edge of the top row.
            let right_edge = layout.history.right();
            let width = right_edge.saturating_sub(point.x);
            SqlTabMessage::SetHistoryWidth { tab_id, width }
        }
    };
    AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(msg))))
}

/// A round triggered by an external message (keyboard/tick). The seed message
/// and any already-available async actions are queued and drained iteratively.
/// Returns the aggregated [`UpdateResult`] so the caller can decide whether the
/// round actually changed rendering state (`dirty`).
fn process_message_round(
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
    queue_result(effect_runner, result, &mut pending);
    drain_async_actions(action_rx, &mut pending);
    drain_pending(effect_runner, &mut pending, state)
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
            if matches!(
                &action,
                SqlAction::SqlTab(SqlTabAction::Results {
                    action: ResultsAction::CommitResult { .. },
                    ..
                })
            ) {
                msgs.push(AppMsg::CloseModal);
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
fn discover_action_to_msg(action: crate::features::discover::effect::DiscoverAction) -> crate::features::discover::msg::DiscoverMessage {
    use crate::features::discover::effect::DiscoverAction as A;
    use crate::features::discover::msg::DiscoverMessage as M;
    match action {
        A::ScanProgress { done, total } => M::ScanProgress { done, total },
        A::ScanComplete { items } => M::ScanComplete { items },
        A::ScanError { error } => M::ScanError { error },
        A::RegisterComplete { count } => M::RegisterComplete { count },
        A::RegisterError { error } => M::RegisterError { error },
    }
}

/// Convert an instance workspace action into the corresponding iw message.
fn iw_action_to_msg(action: crate::features::instance_workspace::effect::IwAction) -> crate::features::instance_workspace::msg::IwMessage {
    use crate::features::instance_workspace::connections::effect::ConnectionsAction as CA;
    use crate::features::instance_workspace::connections::msg::{ConnectionsMessage, ConnectionsMsg};
    use crate::features::instance_workspace::effect::IwAction as IA;
    use crate::features::instance_workspace::msg::IwMessage as IM;
    use crate::features::instance_workspace::overview::effect::OverviewAction as OA;
    use crate::features::instance_workspace::overview::msg::{OverviewMessage, OverviewMsg};
    match action {
        IA::Overview(action) => match action {
            OA::Loaded { instance } => IM::Overview(OverviewMsg::Message(OverviewMessage::Loaded {
                instance,
            })),
            OA::Error { error } => {
                tracing::warn!("iw overview load failed: {error}");
                IM::Overview(OverviewMsg::Message(OverviewMessage::Reload))
            }
        },
        IA::Connections(action) => match action {
            CA::Loaded { connections } => {
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::Loaded { connections }))
            }
            CA::Saved | CA::Deleted => {
                // Reload the connections after a mutation (the connections
                // update fills in the current instance name).
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::Reload))
            }
            CA::Error { error } => {
                tracing::warn!("iw connections op failed: {error}");
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::MoveUp))
            }
        },
        IA::Unregistered { instance } => IM::Unregistered { instance },
    }
}

/// Map a click inside the discover popup to a discover child sub-pane
/// (engine / targets / results), mirroring the discover view's vertical layout
/// and Ctrl+j/k. Returns `None` for clicks outside the popup body (the header
/// / explorer / workspace regions around the popup).
fn discover_subpane_for_click(
    col: u16,
    row: u16,
    explorer_w: u16,
    body_top: u16,
    body_h: u16,
    width: u16,
) -> Option<crate::app_shell::pane::DiscoverPane> {
    use crate::app_shell::pane::DiscoverPane;
    // The discover popup overlays the workspace region: 3/4 of its size,
    // centered (matches `render_modal_popup` in app/view.rs).
    let base_w = width.saturating_sub(explorer_w);
    let w = (base_w * 3) / 4;
    let h = (body_h * 3) / 4;
    if w < 4 || h < 4 {
        return None;
    }
    let px = explorer_w + (base_w - w) / 2;
    let py = body_top + (body_h - h) / 2;
    // Inner area after the 1-row border.
    let inner_x = px + 1;
    let inner_y = py + 1;
    let inner_w = w.saturating_sub(2);
    let inner_h = h.saturating_sub(2);
    if col < inner_x || col >= inner_x + inner_w || row < inner_y || row >= inner_y + inner_h {
        return None;
    }
    let rel = row - inner_y;
    // Discover layout (vertical): engine selector (top 4 rows), then the
    // targets/results body (targets upper half, splitter, results lower half).
    if rel < 4 {
        return Some(DiscoverPane::Engine);
    }
    let body_y = inner_y + 4;
    let body_half = inner_h.saturating_sub(4) / 2;
    if row < body_y + body_half {
        Some(DiscoverPane::Targets)
    } else {
        Some(DiscoverPane::Results)
    }
}

/// Convert a SQL workspace action into the corresponding workspace message,
/// routing editor actions back to the originating tab's editor (the context
/// picker's catalog results).
fn sql_action_to_msg(action: crate::features::sql_workspace::effect::SqlAction) -> crate::features::sql_workspace::msg::SqlMessage {
    use crate::features::sql_workspace::effect::SqlAction as SA;
    use crate::features::sql_workspace::sql_tab::effect::SqlTabAction as STA;
    use crate::features::sql_workspace::sql_tab::editor::effect::EditorAction as EA;
    use crate::features::sql_workspace::sql_tab::editor::context_picker::msg::ContextPickerMsg;
    use crate::features::sql_workspace::sql_tab::editor::msg::{EditorMessage, EditorMsg};
    use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
    use crate::features::sql_workspace::msg::SqlMessage;
    match action {
        SA::SqlTab(STA::Editor { tab_id, action }) => {
            let msg = match action {
                EA::ContextPicker(cp) => EditorMsg::Message(EditorMessage::ContextPicker(
                    ContextPickerMsg::Message(cp_action_to_msg(cp)),
                )),
                EA::CompletionCatalogLoaded(data) => EditorMsg::Message(
                    EditorMessage::CatalogLoaded {
                        tables: data.tables,
                        columns_by_table: data.columns_by_table,
                    },
                ),
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
    }
}

/// Convert a results action into the corresponding results message.
fn results_action_to_msg(action: crate::features::sql_workspace::sql_tab::results::effect::ResultsAction) -> crate::features::sql_workspace::sql_tab::results::msg::ResultsMessage {
    use crate::features::sql_workspace::sql_tab::results::effect::ResultsAction as RA;
    use crate::features::sql_workspace::sql_tab::results::msg::ResultsMessage as M;
    match action {
        RA::ResultReady { result, paginated } => M::SetResult { result, paginated },
        RA::QueryError { message } => {
            tracing::warn!("query failed: {message}");
            M::ClearResult
        }
        RA::CommitResult { ok, message } => {
            tracing::info!("commit ok={ok}: {message}");
            M::ResetSelection
        }
        RA::EditabilityReady { target, blocked } => M::EditabilityReady { target, blocked },
    }
}

/// Convert a context picker action into the corresponding picker message.
fn cp_action_to_msg(action: crate::features::sql_workspace::sql_tab::editor::context_picker::effect::ContextPickerAction) -> crate::features::sql_workspace::sql_tab::editor::context_picker::msg::ContextPickerMessage {
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
fn explorer_action_to_msg(action: crate::features::explorer::effect::ExplorerAction) -> crate::features::explorer::msg::ExplorerMessage {
    use crate::features::explorer::effect::ExplorerAction as EA;
    use crate::features::explorer::instances::effect::InstancesAction as IA;
    use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
    use crate::features::explorer::msg::ExplorerMessage as EM;
    match action {
        EA::Instances(action) => match action {
            IA::InstancesLoaded { instances } => EM::Instances(InstancesMsg::Message(
                InstancesMessage::Loaded { instances },
            )),
            IA::ConnectionsLoaded { instance_idx, connections } => {
                EM::Instances(InstancesMsg::Message(InstancesMessage::ConnectionsLoaded {
                    instance_idx,
                    connections,
                }))
            }
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
        OA::ExtensionsLoaded { database, extensions } => {
            M::ExtensionsLoaded { database, extensions }
        }
        OA::ExtensionsError { database, error } => {
            tracing::warn!("objects extensions load failed for {database}: {error}");
            M::ExtensionsError { database, error }
        }
        OA::ObjectListLoaded { database, schema, kind, items } => {
            M::ObjectListLoaded { database, schema, kind, items }
        }
        OA::ObjectListError { database, schema, kind, error } => {
            tracing::warn!("objects {kind:?} load failed for {database}.{schema}: {error}");
            M::ObjectListError { database, schema, kind, error }
        }
    };
    crate::features::explorer::objects::msg::ObjectsMsg::Message(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sql_workspace::effect::SqlAction;
    use crate::features::sql_workspace::msg::SqlMessage;
    use crate::features::sql_workspace::sql_tab::effect::SqlTabAction;
    use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
    use crate::features::sql_workspace::sql_tab::results::effect::ResultsAction;
    use crate::features::sql_workspace::sql_tab::results::msg::{ResultsMessage, ResultsMsg};
    use crate::features::sql_workspace::sql_tab::results::state::QueryResultData;

    #[test]
    fn discover_click_maps_rows_to_subpanes() {
        use crate::app_shell::pane::DiscoverPane;
        // Fixed layout: width 100, explorer 20, body_top 3, body_h 50.
        // base_w=80 -> popup w=60,h=37 at px=30,py=9; inner x=31,y=10,w=58,h=35.
        let click = |col: u16, row: u16| {
            discover_subpane_for_click(col, row, 20, 3, 50, 100)
        };
        // Engine: top 4 rows of the inner area (inner_y=10 -> rows 10..14).
        assert_eq!(click(40, 11), Some(DiscoverPane::Engine));
        assert_eq!(click(40, 13), Some(DiscoverPane::Engine));
        // Targets: rows [14, body_y+half). body_y=14, half=(35-4)/2=15 -> [14,29).
        assert_eq!(click(40, 15), Some(DiscoverPane::Targets));
        assert_eq!(click(40, 28), Some(DiscoverPane::Targets));
        // Results: rows [29, 45).
        assert_eq!(click(40, 30), Some(DiscoverPane::Results));
        assert_eq!(click(40, 44), Some(DiscoverPane::Results));
        // Outside the popup: header row, explorer column, or beyond the inner
        // area yields None.
        assert_eq!(click(10, 11), None); // explorer column
        assert_eq!(click(40, 1), None); // header row
        assert_eq!(click(99, 11), None); // beyond popup right edge
        assert_eq!(click(40, 45), None); // beyond popup bottom edge
    }

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
    fn sql_results_query_error_clears_result() {
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
        assert!(matches!(msg, ResultsMsg::Message(ResultsMessage::ClearResult)));
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
}
