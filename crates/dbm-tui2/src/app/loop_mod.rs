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
use ratatui::layout::{Constraint, Direction, Layout, Rect};
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

    // Which SQL-tab splitter is being drag-resized, if any. This is transient
    // interaction state that lives only for the lifetime of a drag gesture; it
    // never reaches `AppState` (TEA: state mutations still flow through
    // `update` via split-resize messages).
    let mut split_drag: Option<(usize, crate::features::sql_workspace::sql_tab::layout::SqlSplitter)> = None;

    // The position+time of the most recent left-button press, used to detect a
    // double click (a second press at the same cell within a short window). This
    // lives outside `AppState` because it is transient interaction state, like
    // `split_drag`.
    let mut last_click: Option<(ratatui::layout::Position, std::time::Instant)> = None;

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
    let mut idle_iterations = 0u32;
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
            let footer_h = footer_view::footer_height(&state.footer, size.width);
            terminal.backend_mut().set_exclude_rects(perf_exclude_rects(size, footer_h));
            // Draw the current frame. The perf_monitor feature is passive: the
            // run loop samples each frame here and feeds the smoothed FPS and
            // redundant-redraw ratio from the wrapped backend.
            // `render` reports the focused SQL editor's caret; capture it out of
            // the draw closure (which returns `()`), then place the terminal
            // hardware cursor accordingly (edtui hides its own in-buffer caret).
            let editor_cursor = std::cell::RefCell::new(None);
            tracing::debug!("render: begin terminal.draw");
            terminal.draw(|frame| {
                let c = render(frame, &state);
                *editor_cursor.borrow_mut() = c;
            })?;
            tracing::debug!("render: terminal.draw done");
            crate::common::editor::apply_hardware_cursor(editor_cursor.into_inner())?;
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
                        // A confirm modal is open: clicking its Yes/No button
                        // confirms or cancels, matching the `y`/`n` keys.
                        MouseEventKind::Down(MouseButton::Left)
                            if state
                                .modal
                                .as_ref()
                                .is_some_and(crate::common::view::modal::is_confirm_modal) =>
                        {
                            let size = terminal.size()?;
                            let footer_h =
                                footer_view::footer_height(&state.footer, size.width);
                            let body_top = 3u16;
                            let body_h = size
                                .height
                                .saturating_sub(body_top)
                                .saturating_sub(footer_h);
                            let explorer_w = (size.width.saturating_mul(2) / 10).max(1);
                            let workspace = Rect::new(
                                explorer_w,
                                body_top,
                                size.width.saturating_sub(explorer_w),
                                body_h,
                            );
                            let popup = crate::common::view::modal::confirm_popup_rect(
                                workspace,
                                crate::common::view::modal::confirm_body_rows(
                                    state.modal.as_ref().unwrap(),
                                ),
                            );
                            let buttons =
                                crate::common::view::modal::confirm_buttons(popup);
                            let msg = if buttons.yes_rect.contains(point) {
                                crate::app::input::confirm_yes_msg(
                                    state.modal.as_ref().unwrap(),
                                    &state,
                                )
                            } else if buttons.no_rect.contains(point) {
                                Some(AppMsg::CloseModal)
                            } else {
                                None
                            };
                            if let Some(msg) = msg {
                                let result = process_message_round(
                                    &effect_runner,
                                    &mut action_rx,
                                    msg,
                                    &mut state,
                                );
                                dirty |= result.dirty;
                            }
                        }
                        // Discover's close-confirmation dialog: clicking Yes/No
                        // confirms or cancels closing discover.
                        MouseEventKind::Down(MouseButton::Left)
                            if state.modal.is_none()
                                && matches!(state.focus, Pane::Discover(_))
                                && state.discover.close_confirm =>
                        {
                            let size = terminal.size()?;
                            let footer_h =
                                footer_view::footer_height(&state.footer, size.width);
                            let body_top = 3u16;
                            let body_h = size
                                .height
                                .saturating_sub(body_top)
                                .saturating_sub(footer_h);
                            let explorer_w = (size.width.saturating_mul(2) / 10).max(1);
                            let workspace = Rect::new(
                                explorer_w,
                                body_top,
                                size.width.saturating_sub(explorer_w),
                                body_h,
                            );
                            // Discover's close-confirm body is a single line.
                            let popup = crate::common::view::modal::confirm_popup_rect(
                                workspace,
                                1,
                            );
                            let buttons =
                                crate::common::view::modal::confirm_buttons(popup);
                            let msg = if buttons.yes_rect.contains(point) {
                                Some(AppMsg::Discover(
                                    crate::features::discover::msg::DiscoverMsg::Message(
                                        crate::features::discover::msg::DiscoverMessage::Close,
                                    ),
                                ))
                            } else if buttons.no_rect.contains(point) {
                                Some(AppMsg::Discover(
                                    crate::features::discover::msg::DiscoverMsg::Message(
                                        crate::features::discover::msg::DiscoverMessage::CancelClose,
                                    ),
                                ))
                            } else {
                                None
                            };
                            if let Some(msg) = msg {
                                let result = process_message_round(
                                    &effect_runner,
                                    &mut action_rx,
                                    msg,
                                    &mut state,
                                );
                                dirty |= result.dirty;
                            }
                        }
                        MouseEventKind::Down(MouseButton::Left) if state.modal.is_none() => {
                            // Double-click detection: a second press at the same
                            // cell within the window is treated as a double click.
                            let is_double_click = last_click
                                .as_ref()
                                .is_some_and(|(p, t)| {
                                    *p == point && t.elapsed() < std::time::Duration::from_millis(400)
                                });
                            last_click = Some((point, std::time::Instant::now()));

                            // Map the click to a focus pane by region. The layout
                            // mirrors `app/view.rs`: header (top 3 rows), explorer
                            // (left 20% of the body), workspace (right 80%).
                            let size = terminal.size()?;
                            let footer_h = footer_view::footer_height(&state.footer, size.width);
                            let body_top = 3u16;
                            let body_h =
                                size.height.saturating_sub(body_top).saturating_sub(footer_h);
                            let explorer_w = (size.width.saturating_mul(2) / 10).max(1);

                            // A click outside the open context picker closes it
                            // (mirroring the original dbm), regardless of which
                            // pane the click lands in. The SQL workspace click
                            // handler also closes for clicks in its body, so this
                            // only fires for clicks outside the picker area.
                            if let Some(picker_area) = sql_picker_area_for_hit(size, &state)
                                && !picker_area.contains(point)
                            {
                                let msg = sql_click_msgs(
                                    &state.sql.sql_tab,
                                    crate::features::sql_workspace::sql_tab::view::SqlClickAction::CloseContextPicker,
                                );
                                for m in msg {
                                    let result = process_message_round(&effect_runner, &mut action_rx, m, &mut state);
                                    dirty |= result.dirty;
                                }
                            }

                            // Map the click to a focus pane by region. While the
                            // discover parent pane owns focus, any attempt to
                            // move focus away is rejected by the shell's
                            // FocusChanged handler (update.rs), so clicks outside
                            // the discover popup stay in discover. Clicks inside
                            // the popup switch discover sub-panes below.
                            let target_pane = if mouse.row < body_top {
                                Some(Pane::Header)
                            } else if mouse.row >= body_top + body_h {
                                None
                            } else if mouse.column < explorer_w {
                                // The explorer is a parent pane hosting the
                                // instances (top) and objects (bottom) trees;
                                // map the click row to the matching sub-pane
                                // so mouse navigation agrees with Ctrl+j/k.
                                Some(Pane::Explorer(explorer_pane_for_click(
                                    mouse.row,
                                    body_top,
                                    body_h,
                                )))
                            } else if !state.instance_workspace_open() {
                                Some(Pane::SQLWorkspace)
                            } else {
                                // The workspace region holds the instance pane
                                // (outer border included). A click on its tab bar
                                // switches the active sub-pane (overview /
                                // connections); a click elsewhere keeps the
                                // current sub-pane (default when not focused).
                                let ws = Rect::new(
                                    explorer_w,
                                    body_top,
                                    size.width.saturating_sub(explorer_w),
                                    body_h,
                                );
                                let current = match state.focus {
                                    Pane::InstanceWorkspace(p) => p,
                                    _ => crate::app_shell::nav::IwPane::default(),
                                };
                                Some(Pane::InstanceWorkspace(
                                    crate::features::instance_workspace::view::iw_tab_at(
                                        ws, mouse.column, mouse.row,
                                    )
                                    .unwrap_or(current),
                                ))
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

                            // A single click inside the explorer's instances or
                            // objects tree moves the selection (cursor) to the
                            // clicked row.
                            if mouse.column < explorer_w
                                && let Some(click_msgs) =
                                    explorer_row_click_msgs(size, body_top, body_h, mouse.column, mouse.row, &state)
                            {
                                for click_msg in click_msgs {
                                    let result = process_message_round(
                                        &effect_runner,
                                        &mut action_rx,
                                        click_msg,
                                        &mut state,
                                    );
                                    dirty |= result.dirty;
                                }
                            }

                            // A double click in the explorer activates the node
                            // (Select), like pressing Enter on it — except on an
                            // expand/collapse marker (toggle only) or on blank
                            // space (no node: do nothing, not act on the cursor).
                            if is_double_click
                                && mouse.column < explorer_w
                                && explorer_click_hits_row(
                                    explorer_w,
                                    body_top,
                                    body_h,
                                    mouse.row,
                                    &state,
                                )
                                && !is_explorer_toggle_click(
                                    explorer_w,
                                    body_top,
                                    body_h,
                                    mouse.column,
                                    mouse.row,
                                    &state,
                                )
                            {
                                let select = AppMsg::Explorer(
                                    crate::features::explorer::msg::ExplorerMsg::Message(
                                        match explorer_pane_for_click(mouse.row, body_top, body_h) {
                                            crate::app_shell::nav::ExplorerPane::Instances => {
                                                crate::features::explorer::msg::ExplorerMessage::Instances(
                                                    crate::features::explorer::instances::msg::InstancesMsg::Message(
                                                        crate::features::explorer::instances::msg::InstancesMessage::Select,
                                                    ),
                                                )
                                            }
                                            crate::app_shell::nav::ExplorerPane::Objects => {
                                                crate::features::explorer::msg::ExplorerMessage::Objects(
                                                    crate::features::explorer::objects::msg::ObjectsMsg::Message(
                                                        crate::features::explorer::objects::msg::ObjectsMessage::Select,
                                                    ),
                                                )
                                            }
                                        },
                                    ),
                                );
                                let result = process_message_round(&effect_runner, &mut action_rx, select, &mut state);
                                dirty |= result.dirty;
                            }

                            // A click inside the SQL workspace also routes to a
                            // sub-pane (editor/history/results) or activates the
                            // clicked tab, mirroring the mouse support of the
                            // original dbm.
                            if matches!(target_pane, Some(Pane::SQLWorkspace))
                                && let Some(tab_area) = sql_tab_area_for_hit(size, &state)
                            {
                                if let Some(action) = crate::features::sql_workspace::sql_tab::view::sql_workspace_click(
                                    &state.sql.sql_tab,
                                    tab_area,
                                    mouse.column,
                                    mouse.row,
                                    is_double_click,
                                ) {
                                    for msg in sql_click_msgs(&state.sql.sql_tab, action) {
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

                            // Inside the discover popup: map the click's row to a
                            // discover child pane and switch focus to it. This is
                            // suppressed while the close-confirmation dialog is
                            // open, so clicks on the discover pane behind it are
                            // ignored (only Enter/Esc operate on the dialog).
                            if let Pane::Discover(sub) = state.focus
                                && !state.discover.close_confirm
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
                                // intent: move focus to the Header pane (via the
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
                            // focus and it is actually rendered). The feature
                            // resolves the point to a splitter; the shell only
                            // supplies the area and the coordinates.
                            if state.focus == Pane::SQLWorkspace
                                && let Some(tab_area) = sql_tab_area_for_hit(terminal.size()?, &state)
                            {
                                if let Some((tab_id, splitter)) = crate::features::sql_workspace::sql_tab::view::sql_tab_splitter_at(
                                    &state.sql.sql_tab,
                                    tab_area,
                                    point.x,
                                    point.y,
                                ) {
                                    split_drag = Some((tab_id, splitter));
                                    tracing::debug!(?splitter, "splitter drag started");
                                }
                            }
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if let Some((tab_id, splitter)) = split_drag {
                                tracing::trace!(?splitter, ?point, "drag move begin");
                                let size = terminal.size()?;
                                // The feature resolves the drag to a resize
                                // message; the shell only supplies the area and
                                // the coordinates.
                                if let Some(tab_area) = sql_tab_area_for_hit(size, &state) {
                                    if let Some(msg) = crate::features::sql_workspace::sql_tab::view::sql_tab_splitter_resize_msg(
                                        &state.sql.sql_tab,
                                        tab_area,
                                        tab_id,
                                        splitter,
                                        point.x,
                                        point.y,
                                    ) {
                                        let msg = AppMsg::Sql(
                                            crate::features::sql_workspace::msg::SqlMsg::Message(
                                                crate::features::sql_workspace::msg::SqlMessage::SqlTab(
                                                    crate::features::sql_workspace::sql_tab::msg::SqlTabMsg::Message(msg),
                                                ),
                                            ),
                                        );
                                        tracing::debug!(?msg, "dispatch resize msg");
                                        let result = process_message_round(
                                            &effect_runner,
                                            &mut action_rx,
                                            msg,
                                            &mut state,
                                        );
                                        tracing::debug!("resize msg processed");
                                        dirty |= result.dirty;
                                    }
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
                } else if let Some(Ok(CEvent::Resize(w, _h))) = maybe_event {
                    // Terminal window resized: force a repaint so the layout
                    // recomputes against the new terminal size. Without this,
                    // `terminal.draw` is skipped while idle (no dirty state) and
                    // the rendered frame never catches up with the window size,
                    // unlike the original dbm which redraws on resize.
                    // Also update the cached terminal width so the explorer's
                    // horizontal-scroll can clamp at the content boundary.
                    state.term_width = w;
                    needs_redraw = true;
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
            idle_iterations += 1;
            if idle_iterations >= MAX_IDLE_ITERATIONS {
                tracing::warn!("event loop spin detected; forcing a sleep to break it");
                tokio::time::sleep(Duration::from_millis(8)).await;
                idle_iterations = 0;
            }
        } else {
            idle_iterations = 0;
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

/// Restores the terminal on drop so raw mode / alternate screen / mouse capture
/// are always cleaned up, even if the event loop exits via an error (`?`) or a
/// panic. Without this, an early return leaves the terminal in raw mode and the
/// alternate screen, which makes it look frozen and unresponsive to keys.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture,
            crossterm::event::DisableBracketedPaste
        );
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
    vec![Rect::new(x, size.height - footer_h, size.width - x, footer_h)]
}

/// Compute the SQL tab region (tab bar + child panes) for mouse hit-testing,
/// mirroring `sql_workspace/view.rs` (workspace inner minus its tab footer).
/// Returns `None` when the SQL workspace is not the region being shown.
fn sql_tab_area_for_hit(
    size: ratatui::layout::Size,
    state: &AppState,
) -> Option<ratatui::layout::Rect> {
    if state.modal.is_some()
        || matches!(state.focus, Pane::Discover(_))
        || state.instance_workspace_open()
        || state.sql.sql_tab.tabs.is_empty()
    {
        return None;
    }
    let footer_h = footer_view::footer_height(&state.footer, size.width);
    let body_top = 3u16;
    let body_h = size.height.saturating_sub(body_top).saturating_sub(footer_h);
    if body_h < 3 {
        return None;
    }
    let explorer_w = (size.width.saturating_mul(2) / 10).max(1);
    let workspace_w = size.width.saturating_sub(explorer_w);
    let workspace = Rect::new(explorer_w, body_top, workspace_w, body_h);
    // Outer " SQL Workspace " border (1 col/row).
    let inner = Rect::new(
        workspace.x.saturating_add(1),
        workspace.y.saturating_add(1),
        workspace.width.saturating_sub(2),
        workspace.height.saturating_sub(2),
    );
    // Workspace-level tab footer at the bottom of the inner region.
    let footer_h = crate::common::view::hints::footer_height(
        &crate::common::view::hints::sql_workspace_footer_text(),
        inner.width,
    );
    Some(Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer_h),
    ))
}

/// Compute the context picker overlay rect (in the active tab's editor) for
/// mouse hit-testing, or `None` when the picker is closed / not shown.
fn sql_picker_area_for_hit(
    size: ratatui::layout::Size,
    state: &AppState,
) -> Option<ratatui::layout::Rect> {
    if state.modal.is_some()
        || matches!(state.focus, Pane::Discover(_))
        || state.instance_workspace_open()
        || state.sql.sql_tab.tabs.is_empty()
    {
        return None;
    }
    let tab = state.sql.sql_tab.active_tab()?;
    if !tab.editor.context_picker.open {
        return None;
    }
    let footer_h = footer_view::footer_height(&state.footer, size.width);
    let body_top = 3u16;
    let body_h = size.height.saturating_sub(body_top).saturating_sub(footer_h);
    if body_h < 3 {
        return None;
    }
    let explorer_w = (size.width.saturating_mul(2) / 10).max(1);
    let workspace_w = size.width.saturating_sub(explorer_w);
    let workspace = Rect::new(explorer_w, body_top, workspace_w, body_h);
    // Outer " SQL Workspace " border (1 col/row).
    let inner = Rect::new(
        workspace.x.saturating_add(1),
        workspace.y.saturating_add(1),
        workspace.width.saturating_sub(2),
        workspace.height.saturating_sub(2),
    );
    let footer_h = crate::common::view::hints::footer_height(
        &crate::common::view::hints::sql_workspace_footer_text(),
        inner.width,
    );
    let sql_tab_area = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer_h),
    );
    if sql_tab_area.height < 1 {
        return None;
    }
    // The SQL tab's body sits below its 1-row tab bar.
    let sql_body = Rect::new(
        sql_tab_area.x,
        sql_tab_area.y.saturating_add(1),
        sql_tab_area.width,
        sql_tab_area.height.saturating_sub(1),
    );
    let layout = crate::features::sql_workspace::sql_tab::layout::sql_tab_layout(
        sql_body,
        tab.split_ratio,
        tab.history_pane_width,
    );
    crate::features::sql_workspace::sql_tab::editor::view::context_picker_area(
        layout.editor,
        true,
    )
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
                            crate::features::global_footer::msg::FooterMessage::SetStatus(
                                format!("Query failed: {message}"),
                            ),
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
fn discover_action_to_msg(action: crate::features::discover::effect::DiscoverAction) -> crate::features::discover::msg::DiscoverMessage {
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
                IM::Connections(ConnectionsMsg::Message(ConnectionsMessage::SaveError(error)))
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

/// Compute the two explorer child tree areas (instances top / objects bottom)
/// from the explorer's outer rect, mirroring `explorer/view.rs` (50% + 1-row
/// splitter + 50%, inside the outer border).
fn explorer_child_areas(explorer: ratatui::layout::Rect) -> (ratatui::layout::Rect, ratatui::layout::Rect) {
    // Mirror the explorer render exactly: the two child panes are laid out with
    // `Layout::Vertical([Percentage(50), Length(1), Percentage(50)])` inside the
    // outer border. Computing them the same way here guarantees the click
    // hit-testing uses the same child-pane rectangles the render draws, so a
    // click on a row maps to the same row (no off-by-one from `height/2` vs
    // `Layout` rounding).
    let inner = Rect::new(
        explorer.x.saturating_add(1),
        explorer.y.saturating_add(1),
        explorer.width.saturating_sub(2),
        explorer.height.saturating_sub(2),
    );
    let panes = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Length(1),
            Constraint::Percentage(50),
        ])
        .split(inner);
    (panes[0], panes[2])
}

/// Build the explorer messages for a single click on a visible tree row.
/// Whether the click at `(x, y)` landed on an expand/collapse marker in the
/// explorer's instances/objects tree. Used to suppress the double-click "open"
/// (Select) action on a marker click: clicking the arrow, single or double,
/// must only expand/collapse.
fn is_explorer_toggle_click(
    explorer_w: u16,
    body_top: u16,
    body_h: u16,
    x: u16,
    y: u16,
    state: &AppState,
) -> bool {
    if x >= explorer_w {
        return false;
    }
    let explorer = Rect::new(0, body_top, explorer_w, body_h);
    let (instances_area, objects_area) = explorer_child_areas(explorer);
    match explorer_pane_for_click(y, body_top, body_h) {
        crate::app_shell::nav::ExplorerPane::Instances => {
            crate::features::explorer::instances::view::toggle_at(
                instances_area,
                &state.explorer.instances,
                x,
                y,
            )
            .is_some()
        }
        crate::app_shell::nav::ExplorerPane::Objects => {
            crate::features::explorer::objects::view::toggle_at(
                objects_area,
                &state.explorer.objects,
                x,
                y,
            )
            .is_some()
        }
    }
}

/// Whether the click at `(x, y)` lands on a visible node row in the explorer's
/// instances/objects tree (as opposed to a blank area, a border, or the footer).
/// Used to suppress the double-click "open" (Select) action on blank space:
/// double-clicking a blank region must do nothing, not act on the cursor's node.
fn explorer_click_hits_row(
    explorer_w: u16,
    body_top: u16,
    body_h: u16,
    y: u16,
    state: &AppState,
) -> bool {
    if y < body_top {
        return false;
    }
    let explorer = Rect::new(0, body_top, explorer_w, body_h);
    let (instances_area, objects_area) = explorer_child_areas(explorer);
    match explorer_pane_for_click(y, body_top, body_h) {
        crate::app_shell::nav::ExplorerPane::Instances => {
            crate::features::explorer::instances::view::row_at(
                instances_area,
                &state.explorer.instances,
                y,
            )
            .is_some()
        }
        crate::app_shell::nav::ExplorerPane::Objects => {
            crate::features::explorer::objects::view::row_at(
                objects_area,
                &state.explorer.objects,
                y,
            )
            .is_some()
        }
    }
}

/// Clicking the expand/collapse marker toggles that node's expansion without
/// moving the selection; clicking elsewhere just moves the selection. Returns
/// `None` for clicks on borders/titles/footers.
fn explorer_row_click_msgs(
    size: ratatui::layout::Size,
    body_top: u16,
    body_h: u16,
    x: u16,
    y: u16,
    state: &AppState,
) -> Option<Vec<AppMsg>> {
    use crate::features::explorer::instances::msg::InstancesMessage;
    use crate::features::explorer::objects::msg::ObjectsMessage;
    let explorer_w = (size.width.saturating_mul(2) / 10).max(1);
    if x >= explorer_w {
        return None;
    }
    let explorer = Rect::new(0, body_top, explorer_w, body_h);
    let (instances_area, objects_area) = explorer_child_areas(explorer);
    let pane = explorer_pane_for_click(y, body_top, body_h);
    match pane {
        crate::app_shell::nav::ExplorerPane::Instances => {
            let inst = &state.explorer.instances;
            let row = crate::features::explorer::instances::view::row_at(instances_area, inst, y)?;
            let jump = instances_msg(InstancesMessage::JumpTo { row });
            // Clicking the expand/collapse marker on an instance row toggles its
            // expansion (not Select, which would open the workspace). Need the
            // row's instance index and current state.
            if crate::features::explorer::instances::view::toggle_at(instances_area, inst, x, y).is_some() {
                // Clicking the expand/collapse marker toggles that instance's
                // expansion without moving the cursor (no `jump`).
                return Some(vec![instances_msg(InstancesMessage::ToggleExpandAt { row })]);
            }
            Some(vec![jump])
        }
        crate::app_shell::nav::ExplorerPane::Objects => {
            let objs = &state.explorer.objects;
            let row = crate::features::explorer::objects::view::row_at(objects_area, objs, y)?;
            let jump = objects_msg(ObjectsMessage::JumpTo { row });
            // Clicking the expand/collapse marker toggles that database/group's
            // expansion without moving the cursor (no `jump`).
            if crate::features::explorer::objects::view::toggle_at(objects_area, objs, x, y).is_some() {
                return Some(vec![objects_msg(ObjectsMessage::ToggleExpandAt { row })]);
            }
            Some(vec![jump])
        }
    }
}

fn instances_msg(m: crate::features::explorer::instances::msg::InstancesMessage) -> AppMsg {
    AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
        crate::features::explorer::msg::ExplorerMessage::Instances(
            crate::features::explorer::instances::msg::InstancesMsg::Message(m),
        ),
    ))
}

fn objects_msg(m: crate::features::explorer::objects::msg::ObjectsMessage) -> AppMsg {
    AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
        crate::features::explorer::msg::ExplorerMessage::Objects(
            crate::features::explorer::objects::msg::ObjectsMsg::Message(m),
        ),
    ))
}

/// Map a click row inside the explorer column to an explorer child sub-pane
/// (instances on top / objects on the bottom), mirroring the explorer view's
/// vertical layout and Ctrl+j/k. The row is relative to the explorer's outer
/// border (top row) and body height, matching the layout in `explorer/view.rs`.
fn explorer_pane_for_click(
    row: u16,
    body_top: u16,
    body_h: u16,
) -> crate::app_shell::nav::ExplorerPane {
    use crate::app_shell::nav::ExplorerPane;
    // Use the same `Layout` as the render and `explorer_child_areas` so the
    // instances/objects boundary matches exactly (no `height/2` vs `Layout`
    // rounding drift).
    let explorer = Rect::new(0, body_top, 1, body_h);
    let (instances, _objects) = explorer_child_areas(explorer);
    if row <= instances.y.saturating_add(instances.height) {
        ExplorerPane::Instances
    } else {
        ExplorerPane::Objects
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
) -> Option<crate::app_shell::nav::DiscoverPane> {
    use crate::app_shell::nav::DiscoverPane;
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

/// Build the workspace messages for a SQL click action. A double-click on a
/// picker row yields both a cursor jump and an apply, so a `Vec` is returned.
fn sql_click_msgs(
    sql: &crate::features::sql_workspace::sql_tab::state::SqlTabState,
    action: crate::features::sql_workspace::sql_tab::view::SqlClickAction,
) -> Vec<AppMsg> {
    use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
    use crate::features::sql_workspace::sql_tab::editor::context_picker::msg::{
        ContextPickerMessage, ContextPickerMsg,
    };
    use crate::features::sql_workspace::sql_tab::editor::msg::{EditorMessage, EditorMsg};
    use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
    use crate::features::sql_workspace::sql_tab::view::SqlClickAction;

    let tab_id = |active: Option<usize>| active.and_then(|i| sql.tabs.get(i)).map(|t| t.session.id);
    let editor_msg = |tab_id: usize, m: EditorMessage| {
        AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
            SqlTabMessage::Editor {
                tab_id,
                msg: EditorMsg::Message(m),
            },
        ))))
    };
    let picker = |tab_id: usize, m: ContextPickerMessage| {
        editor_msg(tab_id, EditorMessage::ContextPicker(ContextPickerMsg::Message(m)))
    };
    let close = |tab_id: usize| picker(tab_id, ContextPickerMessage::Close);

    match action {
        SqlClickAction::FocusSubPane(focus) => {
            vec![AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Focus(focus),
            ))))]
        }
        SqlClickAction::ActivateTab(visible_idx) => vec![AppMsg::Sql(SqlMsg::Message(
            SqlMessage::SqlTab(SqlTabMsg::Message(SqlTabMessage::Tab(visible_idx))),
        ))],
        SqlClickAction::CloseContextPicker => {
            let Some(tab_id) = tab_id(sql.active_tab) else {
                return Vec::new();
            };
            vec![close(tab_id)]
        }
        SqlClickAction::OpenContextPicker(column) => {
            let Some(tab) = sql.active_tab() else {
                return Vec::new();
            };
            let tab_id = tab.session.id;
            vec![picker(
                tab_id,
                ContextPickerMessage::Open {
                    column,
                    instance: tab.session.instance.clone().unwrap_or_default(),
                    connection: tab.session.connection.clone().unwrap_or_default(),
                    database: tab.session.database.clone().unwrap_or_default(),
                    schema: tab.session.schema.clone().unwrap_or_default(),
                },
            )]
        }
        SqlClickAction::ContextPickerHit { column, cursor, double } => {
            let Some(tab_id) = tab_id(sql.active_tab) else {
                return Vec::new();
            };
            let mut msgs = vec![picker(
                tab_id,
                ContextPickerMessage::SetCursor { column, cursor },
            )];
            if double {
                msgs.push(picker(tab_id, ContextPickerMessage::Apply));
            }
            msgs
        }
        SqlClickAction::ContextPickerColumn(column) => {
            let Some(tab_id) = tab_id(sql.active_tab) else {
                return Vec::new();
            };
            vec![picker(tab_id, ContextPickerMessage::MoveColumn(column))]
        }
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
    fn explorer_child_areas_stack_trees() {
        let explorer = Rect::new(0, 3, 40, 21);
        let (instances, objects) = explorer_child_areas(explorer);
        // Outer border: inner is (1,4,38,19); half = 9.
        assert_eq!(instances, Rect::new(1, 4, 38, 9));
        // Objects start after the 1-row splitter.
        assert_eq!(objects.y, instances.y + instances.height + 1);
        assert_eq!(objects.width, 38);
    }

    #[test]
    fn explorer_click_maps_rows_to_instances_objects() {
        use crate::app_shell::nav::ExplorerPane;
        // body_top=3, body_h=20. The boundary derives from the same `Layout` the
        // render uses, so it is exact (no `height/2` vs `Layout` rounding drift).
        let (instances, _objects) = explorer_child_areas(Rect::new(0, 3, 1, 20));
        let boundary = instances.y.saturating_add(instances.height);
        assert_eq!(explorer_pane_for_click(5, 3, 20), ExplorerPane::Instances);
        assert_eq!(
            explorer_pane_for_click(boundary, 3, 20),
            ExplorerPane::Instances
        );
        assert_eq!(
            explorer_pane_for_click(boundary.saturating_add(1), 3, 20),
            ExplorerPane::Objects
        );
        assert_eq!(explorer_pane_for_click(21, 3, 20), ExplorerPane::Objects);
    }

    #[test]
    fn explorer_marker_click_toggles_without_moving_the_cursor() {
        use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};

        // One instance row (cursor on it) so the marker click hits an instance.
        let mut state = AppState::default();
        state.explorer.instances.set_instances(vec![dbm_store::ManagedInstance {
            id: "a".into(),
            fingerprint: "a".into(),
            name: "a".into(),
            engine: dbm_core::Engine::Postgres,
            host: "h".into(),
            port: 1,
            socket_path: None,
            data_dir: None,
            env_label: None,
            registered_at: "now".into(),
            version_full: None,
            version_short: None,
            version_checked_at: None,
            lifecycle_status: None,
            lifecycle_checked_at: None,
            lifecycle_detail: None,
        }]);
        state.explorer.instances.cursor = 0;

        // Layout: size 100x50, body_top=3, explorer_w=20.
        // explorer_child_areas(Rect(0,3,20,50)) -> instances_area = Rect(1,4,...).
        // Instance marker is the 2nd body char: x = instances_area.x+2 = 3.
        // Row 0 is the first body row: y = instances_area.y+1 = 5.
        let msgs = explorer_row_click_msgs(
            ratatui::layout::Size::new(100, 50),
            3,
            50,
            3,
            5,
            &state,
        )
        .expect("marker click maps to a row");
        let has_toggle = msgs.iter().any(|m| matches!(
            m,
            AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
                InstancesMsg::Message(InstancesMessage::ToggleExpandAt { row: 0 })
            )))
        ));
        assert!(has_toggle, "marker click toggles expansion: {msgs:?}");
        let has_jump = msgs.iter().any(|m| matches!(
            m,
            AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
                InstancesMsg::Message(InstancesMessage::JumpTo { .. })
            )))
        ));
        assert!(
            !has_jump,
            "marker click must not move the cursor: {msgs:?}"
        );
    }

    #[test]
    fn is_explorer_toggle_click_detects_the_marker_column() {
        // Same layout as the marker test: size 100x50, explorer_w=20, body_top=3,
        // instances_area = Rect(1,4,...). Instance marker = x=3, row0 y=5.
        let mut state = AppState::default();
        state.explorer.instances.set_instances(vec![dbm_store::ManagedInstance {
            id: "a".into(),
            fingerprint: "a".into(),
            name: "a".into(),
            engine: dbm_core::Engine::Postgres,
            host: "h".into(),
            port: 1,
            socket_path: None,
            data_dir: None,
            env_label: None,
            registered_at: "now".into(),
            version_full: None,
            version_short: None,
            version_checked_at: None,
            lifecycle_status: None,
            lifecycle_checked_at: None,
            lifecycle_detail: None,
        }]);
        // Marker column (x=3) counts as a toggle click.
        assert!(is_explorer_toggle_click(20, 3, 50, 3, 5, &state));
        // The label/text column (x=8) does not.
        assert!(!is_explorer_toggle_click(20, 3, 50, 8, 5, &state));
        // Outside the explorer is never a toggle click.
        assert!(!is_explorer_toggle_click(20, 3, 50, 25, 5, &state));
    }

    #[test]
    fn explorer_click_hits_row_distinguishes_nodes_from_blank() {
        // One instance row (row 0) at y=5 (body_top=3, explorer_w=20).
        let mut state = AppState::default();
        state.explorer.instances.set_instances(vec![dbm_store::ManagedInstance {
            id: "a".into(),
            fingerprint: "a".into(),
            name: "a".into(),
            engine: dbm_core::Engine::Postgres,
            host: "h".into(),
            port: 1,
            socket_path: None,
            data_dir: None,
            env_label: None,
            registered_at: "now".into(),
            version_full: None,
            version_short: None,
            version_checked_at: None,
            lifecycle_status: None,
            lifecycle_checked_at: None,
            lifecycle_detail: None,
        }]);
        // y=5 is row 0 (A): a real node row.
        assert!(explorer_click_hits_row(20, 3, 50, 5, &state));
        // Blank area below the only node (y=6+) is not a node row.
        assert!(!explorer_click_hits_row(20, 3, 50, 8, &state));
        // The explorer border/title row (y=3) is not a node row.
        assert!(!explorer_click_hits_row(20, 3, 50, 3, &state));
    }

    #[test]
    fn instances_arrow_click_targets_the_collapsed_node_not_the_active_one() {
        use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        // The user's repro: after restart, instance A is collapsed-unloaded and
        // the active workspace is on instance B. A click on A's arrow must
        // toggle A (row 0), not drift to B (row 1).
        let mut state = AppState::default();
        state.explorer.instances.set_instances(vec![
            dbm_store::ManagedInstance {
                id: "a".into(),
                fingerprint: "a".into(),
                name: "a".into(),
                engine: dbm_core::Engine::Postgres,
                host: "h".into(),
                port: 1,
                socket_path: None,
                data_dir: None,
                env_label: None,
                registered_at: "now".into(),
                version_full: None,
                version_short: None,
                version_checked_at: None,
                lifecycle_status: None,
                lifecycle_checked_at: None,
                lifecycle_detail: None,
            },
            dbm_store::ManagedInstance {
                id: "b".into(),
                fingerprint: "b".into(),
                name: "b".into(),
                engine: dbm_core::Engine::Postgres,
                host: "h".into(),
                port: 1,
                socket_path: None,
                data_dir: None,
                env_label: None,
                registered_at: "now".into(),
                version_full: None,
                version_short: None,
                version_checked_at: None,
                lifecycle_status: None,
                lifecycle_checked_at: None,
                lifecycle_detail: None,
            },
        ]);
        state.explorer.instances.nodes[0].expanded = false; // A collapsed
        state.explorer.instances.set_active_instance(1); // active on B
        state.explorer.instances.cursor = 0; // cursor on A
        // Layout: size 100x50, body_top=3, body_h=45, explorer_w=20.
        // instances_area.y=4 -> first row (A) at y=5; marker x=3.
        let msgs = explorer_row_click_msgs(
            ratatui::layout::Size::new(100, 50),
            3,
            45,
            3,
            5,
            &state,
        )
        .expect("click on A's arrow maps to a row");
        assert!(
            msgs.iter().any(|m| matches!(
                m,
                AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
                    InstancesMsg::Message(InstancesMessage::ToggleExpandAt { row: 0 })
                )))
            )),
            "expected ToggleExpandAt row 0 (A), got {msgs:?}"
        );
    }

    #[test]
    fn instances_arrow_renders_at_the_row_click_math_expects() {
        use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        // Render the real explorer at the same geometry the click handler uses,
        // then confirm the rendered first instance row is at the y that maps to
        // row 0. This catches any render/row_at drift for the restart scenario
        // (A collapsed-unloaded, active B).
        let mut state = AppState::default();
        state.explorer.instances.set_instances(vec![dbm_store::ManagedInstance {
            id: "a".into(),
            fingerprint: "a".into(),
            name: "a".into(),
            engine: dbm_core::Engine::Postgres,
            host: "h".into(),
            port: 1,
            socket_path: None,
            data_dir: None,
            env_label: None,
            registered_at: "now".into(),
            version_full: None,
            version_short: None,
            version_checked_at: None,
            lifecycle_status: None,
            lifecycle_checked_at: None,
            lifecycle_detail: None,
        }]);
        state.explorer.instances.nodes[0].expanded = false;
        state.explorer.instances.set_active_instance(0);
        state.explorer.instances.cursor = 0;
        let theme = crate::common::view::theme::dracula();
        let mut terminal = Terminal::new(TestBackend::new(100, 50)).unwrap();
        terminal
            .draw(|frame| {
                let theme = theme.clone();
                // Explorer area: Rect(0, 3, 20, 45) matches body_top=3, body_h=45.
                crate::features::explorer::view::render(
                    frame,
                    &theme,
                    ratatui::layout::Rect::new(0, 3, 20, 45),
                    &state.explorer,
                    true,
                );
            })
            .unwrap();
        // Find the y of the first instance row (contains "a" in the instances
        // column, not the "Explorer" title).
        let buf = terminal.backend().buffer();
        let mut first_y = None;
        for y in 0..50 {
            let mut line = String::new();
            for x in 0..20 {
                line.push_str(buf[(x, y)].symbol());
            }
            if line.contains("a") && !line.contains("Explorer") && !line.contains("Instances") {
                first_y = Some(y as u16);
                break;
            }
        }
        let y = first_y.expect("first instance row rendered");
        // The click handler maps this rendered y (with marker x=3) to row 0.
        let msgs = explorer_row_click_msgs(
            ratatui::layout::Size::new(100, 50),
            3,
            45,
            3,
            y,
            &state,
        )
        .expect("click on rendered arrow maps to a row");
        assert!(
            msgs.iter().any(|m| matches!(
                m,
                AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
                    InstancesMsg::Message(InstancesMessage::ToggleExpandAt { row: 0 })
                )))
            )),
            "rendered row {y} must map to ToggleExpandAt row 0, got {msgs:?}"
        );
    }

    #[test]
    fn discover_click_maps_rows_to_subpanes() {
        use crate::app_shell::nav::DiscoverPane;
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
        assert!(!state.discover.scanning, "scanning must clear on completion");
        assert_eq!(state.discover.scan_progress, None);
    }

    #[test]
    fn full_app_render_does_not_panic_at_extreme_history_split_widths() {
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
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
                let tab = &mut state.sql.sql_tab.tabs[0];
                tab.focus = SqlFocus::History;
                tab.history_pane_width = history_w;
                tab.history.detail_pane_width = detail_w;
                tab.history
                    .store
                    .record_success("inst", "c1", "SELECT * FROM users");
                // Give the editor real SQL content so it is exercised at the
                // narrow width the widened history zone leaves it.
                tab.editor = crate::features::sql_workspace::sql_tab::editor::state::EditorState::with_sql(
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
