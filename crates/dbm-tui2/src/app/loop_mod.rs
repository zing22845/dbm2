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

use std::time::{Duration, Instant};

use crossterm::event::{Event as CEvent, EventStream, KeyCode};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use tokio::sync::mpsc;

use crate::app::action::Action;
use crate::app::mouse::dispatch::handle_mouse_event;
use crate::app::mouse::drag::clear_active_drags;
use crate::app::mouse::state::{MouseInteraction, MouseOutcome};
use crate::app::msg::AppMsg;
use crate::app::round::{process_action_round, process_message_round, queue_result};
use crate::app::state::AppState;
use crate::app::view::render;
use crate::app_shell::effect::EffectRunner;
use crate::features::global_footer::view as footer_view;
use crate::features::perf_monitor::backend::CountingBackend;

const TICK_RATE: Duration = Duration::from_millis(250);

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
    let mut mouse_interaction = MouseInteraction::default();

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
            // Keep each splitter's clamp bounds on the live layout. TEA: the run
            // loop forwards a shell message and `update` owns the write.
            crate::app::update::update_unchecked(
                AppMsg::Shell(crate::app_shell::msg::ShellMsg::RefreshSplitterBounds),
                &mut state,
            );
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
                    let size = terminal.size()?;
                    match handle_mouse_event(
                        mouse,
                        size,
                        &mut state,
                        &effect_runner,
                        &mut action_rx,
                        &mut mouse_interaction,
                    )? {
                        // A debounced wheel tick: skip the rest of this loop
                        // iteration, exactly as the inlined `continue` did.
                        MouseOutcome::Continue => continue,
                        MouseOutcome::Handled { repaint } => {
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
                    // TEA: the resize flows through `update`, which records the
                    // cached size and marks the round dirty.
                    crate::app::update::update_unchecked(
                        AppMsg::Shell(crate::app_shell::msg::ShellMsg::TermResized {
                            width: w,
                            height: h,
                        }),
                        &mut state,
                    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_shell::pane::Pane;

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
