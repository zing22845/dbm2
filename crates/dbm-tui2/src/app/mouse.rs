//! Mouse input: turning a crossterm mouse event into app messages.
//!
//! Everything here is *input plumbing* — hit-testing a screen position against
//! the rendered layout and dispatching the resulting messages through the
//! central `update`. No state is mutated directly: every change goes through a
//! message, so the TEA data flow (`input -> msg -> update -> view`) stays
//! intact even for drags and scrollbar grabs.
//!
//! The module owns the two things that would otherwise bloat the run loop:
//!
//! - the per-event-kind branches (press / drag / release / move / wheel), which
//!   used to be a ~2200-line `match` inside `run_event_loop`;
//! - the transient gesture state [`MouseInteraction`], which must survive
//!   across events but deliberately stays out of `AppState`.

use std::time::Instant;

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Position, Rect};
use tokio::sync::mpsc;

use crate::app::action::Action;
use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app_shell::effect::EffectRunner;
use crate::app_shell::pane::Pane;
use crate::common::view::pane_scrollbar::{ActiveScrollbar, ScrollbarDrag};
use crate::features::global_footer::view as footer_view;
use crate::features::header::msg::{HeaderMessage, HeaderMsg};
use crate::features::perf_monitor::backend::CountingBackend;

use super::loop_mod::process_message_round;

/// The terminal the run loop drives: a cell-change counting backend wrapping
/// crossterm. Only its size is needed here (to hit-test against the layout).
type AppTerminal = Terminal<CountingBackend<CrosstermBackend<std::io::Stdout>>>;

/// Trackpad wheel debounce window: a macOS trackpad emits a burst of ticks per
/// physical gesture, so ticks closer together than this collapse into one.
const WHEEL_DEBOUNCE_MS: u128 = 15;

/// End every in-progress held-button drag at once: the active scrollbar, all
/// splitter drag flags, and the results column-resize drag.
///
/// Called from the three places a drag is known to be over:
/// - a real `MouseEventKind::Up`,
/// - a `FocusLost` (the button was released *outside* the window, so no `Up`
///   was ever delivered — otherwise the highlight would stay stuck),
/// - a fresh `Down` that proves the previous release was missed.
///
/// Centralizing the clear keeps it in one place instead of three duplicated
/// blocks (and mirrors the original dbm, which clears scrollbar/splitter/
/// column-resize together in its single `Up` arm).
pub(crate) fn clear_active_drags(state: &mut AppState) {
    state.scrollbar_drag = None;
    state.splitter_hover.set_dragging_flags([false; 7]);
    state.splitter_hover.results_col_resize_drag = None;
}

/// The splitter a mouse press resolved to, i.e. the one being drag-resized.
///
/// A single `Option` (rather than one flag per splitter) makes a drag mutually
/// exclusive **by construction**: `Down` arms at most one target, and `Drag`
/// dispatches on that target alone, so a drag can never resize a second split.
///
/// Each variant also fixes the axis its drag reads — a vertical splitter owns a
/// width (reads `x`), a horizontal one owns a height (reads `y`) — so dragging
/// a splitter cannot move the other dimension either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SplitterDrag {
    /// App-level Explorer / workspace splitter (vertical: width, reads `x`).
    App,
    /// Explorer instances / objects splitter (horizontal: height, reads `y`).
    Explorer,
    /// Discover targets / results splitter (horizontal: height, reads `y`).
    Discover,
    /// A SQL-tab splitter inside the given tab (axis depends on the variant).
    Sql(
        usize,
        crate::features::sql_workspace::sql_tab::splitter::view::SqlSplitter,
    ),
}
/// Transient mouse-gesture state, owned by the run loop.
///
/// These values describe *the gesture in progress*, not the application: which
/// splitter is being dragged, where the last press landed (double-click
/// detection), the last wheel tick (trackpad debounce) and the event-loop spin
/// watchdog. They must survive across events, but they are not application
/// state — a TEA `update` never sees them — so they stay out of `AppState`.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct MouseInteraction {
    /// The splitter currently being drag-resized, if any. Exactly one can be
    /// armed at a time, which is what stops a drag resizing two splits.
    pub(crate) splitter_drag: Option<SplitterDrag>,
    /// Position and time of the most recent left press, for double clicks.
    pub(crate) last_click: Option<(Position, Instant)>,
    /// `(time, direction, horizontal)` of the last wheel tick, for debouncing.
    pub(crate) last_wheel: Option<(Instant, i32, bool)>,
    /// Consecutive event-loop iterations without a repaint (spin watchdog).
    pub(crate) idle_iterations: u32,
}

/// What the run loop should do once a mouse event has been handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MouseOutcome {
    /// The event was a debounced wheel tick: skip the rest of this loop
    /// iteration (the watchdog counter has already been reset).
    Continue,
    /// The event was handled; repaint only if `repaint` is set.
    Handled { repaint: bool },
}

/// Handle one mouse event: hit-test its position against the rendered layout
/// and dispatch the resulting messages through `update`.
///
/// `interaction` carries the gesture state across events (see
/// [`MouseInteraction`]); it is written back on every exit path.
pub(crate) fn handle_mouse_event(
    mouse: MouseEvent,
    terminal: &mut AppTerminal,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    interaction: &mut MouseInteraction,
) -> anyhow::Result<MouseOutcome> {
    tracing::trace!(
        kind = ?mouse.kind,
        col = mouse.column,
        row = mouse.row,
        "mouse event received"
    );
    let point = Position::new(mouse.column, mouse.row);
    // Take the gesture state by value: every field is `Copy` and this function
    // has exactly one exit point below, so it is always written back.
    let mut splitter_drag = interaction.splitter_drag;
    let mut last_click = interaction.last_click;
    let mut last_wheel = interaction.last_wheel;
    let mut idle_iterations = interaction.idle_iterations;
    // Aggregated across the dispatched messages below: the round repaints only
    // if one of them changed rendered state.
    let mut dirty = false;

    // A fresh mouse-button press while a drag is still recorded means the
    // previous `Up` was missed (e.g. released outside the window without the
    // terminal losing focus). Drop the stale drag so its highlight cannot stay
    // stuck across the next redraw. A second `Down` can never be legitimate
    // while a drag is active — crossterm never delivers one without an
    // intervening `Up`.
    if matches!(mouse.kind, MouseEventKind::Down(_))
        && (state.scrollbar_drag.is_some()
            || splitter_drag.is_some()
            || state.splitter_hover.results_col_resize_drag.is_some()
            || state.splitter_hover.dragging_flags().iter().any(|&f| f))
    {
        splitter_drag = None;
        state.scrollbar_drag = None;
        state.splitter_hover.set_dragging_flags([false; 7]);
        state.splitter_hover.results_col_resize_drag = None;
        dirty = true;
    }

    // Set when a wheel tick is swallowed by the trackpad debounce: the run loop
    // must then skip the rest of its iteration.
    let mut skip_iteration = false;
    'mouse: {
        match mouse.kind {
            // A confirm modal is open: clicking its Yes/No button
            // confirms or cancels, matching the `y`/`n` keys.
            MouseEventKind::Down(MouseButton::Left)
                if state
                    .modal
                    .as_ref()
                    .is_some_and(crate::common::view::modal::is_confirm_modal) =>
            {
                handle_confirm_modal_click(
                    terminal,
                    state,
                    effect_runner,
                    action_rx,
                    point,
                    &mut dirty,
                )?
            }
            // Discover's close-confirmation dialog: clicking Yes/No
            // confirms or cancels closing discover.
            MouseEventKind::Down(MouseButton::Left)
                if state.modal.is_none()
                    && matches!(state.focus, Pane::Discover(_))
                    && state.discover.close_confirm =>
            {
                handle_discover_close_confirm_click(
                    terminal,
                    state,
                    effect_runner,
                    action_rx,
                    point,
                    &mut dirty,
                )?
            }
            MouseEventKind::Down(MouseButton::Left) if state.modal.is_none() => handle_down(
                &mouse,
                terminal,
                state,
                effect_runner,
                action_rx,
                point,
                &mut dirty,
                &mut splitter_drag,
                &mut last_click,
            )?,
            MouseEventKind::Drag(MouseButton::Left) => handle_drag(
                terminal,
                state,
                effect_runner,
                action_rx,
                point,
                &mut dirty,
                &mut splitter_drag,
            )?,
            MouseEventKind::Up(MouseButton::Left) => {
                handle_up(&mouse, terminal, state, &mut dirty, &mut splitter_drag)?
            }
            MouseEventKind::Moved => {
                handle_moved(&mouse, terminal, state, &mut dirty, &mut splitter_drag)?
            }

            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if matches!(
                    state.focus,
                    Pane::Discover(crate::app_shell::nav::DiscoverPane::Targets)
                ) && !state.discover.close_confirm =>
            {
                // Wheel debounce: collapse macOS trackpad burst events
                // so each physical tick maps to one MoveUp/Down.
                // Shift+wheel is the platform convention for
                // horizontal scrolling; some terminals instead
                // report the gesture natively as ScrollLeft/Right.
                let (horizontal, dir) = wheel_axis(mouse.kind, mouse.modifiers);
                let now = std::time::Instant::now();
                if let Some((t, d, h)) = last_wheel
                    && d == dir
                    && h == horizontal
                    && now.duration_since(t).as_millis() < WHEEL_DEBOUNCE_MS
                {
                    idle_iterations = 0;
                    skip_iteration = true;
                    break 'mouse;
                }
                last_wheel = Some((now, dir, horizontal));

                let size = terminal.size()?;
                let footer_h = footer_view::footer_height(&state.footer, size.width);
                let body_top = 3u16;
                let body_h = size
                    .height
                    .saturating_sub(body_top)
                    .saturating_sub(footer_h);
                if let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state)
                    && let discover_popup =
                        crate::common::view::modal::popup_rect(workspace, 75, 75)
                    && let body = crate::features::discover::view::discover_body_area(
                        discover_popup,
                        &state.discover,
                    )
                    && !body.is_empty()
                    && let layout = crate::features::discover::splitter::view::discover_body_layout(
                        body,
                        state.discover.splitter.targets_height,
                    )
                    && layout.targets.contains(point)
                {
                    use crate::features::discover::targets::msg::{TargetsMessage, TargetsMsg};
                    // No horizontal scroll in this pane, so a
                    // shift-wheel falls through to the vertical axis.
                    let msg = match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                                crate::features::discover::msg::DiscoverMessage::Targets(
                                    TargetsMsg::Message(TargetsMessage::MoveUp),
                                ),
                            ))
                        }
                        MouseEventKind::ScrollDown => {
                            AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                                crate::features::discover::msg::DiscoverMessage::Targets(
                                    TargetsMsg::Message(TargetsMessage::MoveDown),
                                ),
                            ))
                        }
                        _ => unreachable!(),
                    };
                    let result = process_message_round(effect_runner, action_rx, msg, state);
                    dirty |= result.dirty;
                }
            }
            // Scroll wheel: route to the discover results pane
            // when focus is on Results and mouse is inside it.
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if matches!(
                    state.focus,
                    Pane::Discover(crate::app_shell::nav::DiscoverPane::Results)
                ) && !state.discover.close_confirm =>
            {
                // Shift+wheel is the platform convention for
                // horizontal scrolling; some terminals instead
                // report the gesture natively as ScrollLeft/Right.
                let (horizontal, dir) = wheel_axis(mouse.kind, mouse.modifiers);
                let now = std::time::Instant::now();
                if let Some((t, d, h)) = last_wheel
                    && d == dir
                    && h == horizontal
                    && now.duration_since(t).as_millis() < WHEEL_DEBOUNCE_MS
                {
                    idle_iterations = 0;
                    skip_iteration = true;
                    break 'mouse;
                }
                last_wheel = Some((now, dir, horizontal));

                let size = terminal.size()?;
                let footer_h = footer_view::footer_height(&state.footer, size.width);
                let body_top = 3u16;
                let body_h = size
                    .height
                    .saturating_sub(body_top)
                    .saturating_sub(footer_h);
                let point = ratatui::layout::Position::new(mouse.column, mouse.row);
                if let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state)
                    && let discover_popup =
                        crate::common::view::modal::popup_rect(workspace, 75, 75)
                    && let body = crate::features::discover::view::discover_body_area(
                        discover_popup,
                        &state.discover,
                    )
                    && !body.is_empty()
                    && let layout = crate::features::discover::splitter::view::discover_body_layout(
                        body,
                        state.discover.splitter.targets_height,
                    )
                    && layout.results.contains(point)
                {
                    use crate::features::discover::results::msg::{ResultsMessage, ResultsMsg};
                    // No horizontal scroll in this pane, so a
                    // shift-wheel falls through to the vertical axis.
                    let msg = match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                                crate::features::discover::msg::DiscoverMessage::Results(
                                    ResultsMsg::Message(ResultsMessage::MoveUp),
                                ),
                            ))
                        }
                        MouseEventKind::ScrollDown => {
                            AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                                crate::features::discover::msg::DiscoverMessage::Results(
                                    ResultsMsg::Message(ResultsMessage::MoveDown),
                                ),
                            ))
                        }
                        _ => unreachable!(),
                    };
                    let result = process_message_round(effect_runner, action_rx, msg, state);
                    dirty |= result.dirty;
                }
            }
            // Scroll wheel: route to explorer objects pane when
            // focus is on Explorer Objects and mouse is inside it.
            MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
                if matches!(
                    state.focus,
                    Pane::Explorer(crate::app_shell::nav::ExplorerPane::Objects)
                ) =>
            {
                // Shift+wheel is the platform convention for
                // horizontal scrolling; some terminals instead
                // report the gesture natively as ScrollLeft/Right.
                let (horizontal, dir) = wheel_axis(mouse.kind, mouse.modifiers);
                let now = std::time::Instant::now();
                if let Some((t, d, h)) = last_wheel
                    && d == dir
                    && h == horizontal
                    && now.duration_since(t).as_millis() < WHEEL_DEBOUNCE_MS
                {
                    idle_iterations = 0;
                    skip_iteration = true;
                    break 'mouse;
                }
                last_wheel = Some((now, dir, horizontal));

                let size = terminal.size()?;
                let footer_h = footer_view::footer_height(&state.footer, size.width);
                let body_top = 3u16;
                let body_h = size
                    .height
                    .saturating_sub(body_top)
                    .saturating_sub(footer_h);
                let point = ratatui::layout::Position::new(mouse.column, mouse.row);
                if let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
                    && let (_inst_area, objs_area) =
                        explorer_child_areas(explorer, state.explorer.splitter.instances_height)
                    && objs_area.contains(point)
                {
                    use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
                    use crate::features::explorer::objects::msg::{ObjectsMessage, ObjectsMsg};
                    // Shift+wheel scrolls the tree sideways; a bare
                    // wheel keeps moving the cursor up/down.
                    let msg = if horizontal {
                        AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Objects(
                            ObjectsMsg::Message(ObjectsMessage::ScrollHorizontal {
                                delta: (dir * WHEEL_H_STEP) as i16,
                                term_width: size.width,
                            }),
                        )))
                    } else if dir < 0 {
                        AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Objects(
                            ObjectsMsg::Message(ObjectsMessage::MoveUp),
                        )))
                    } else {
                        AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Objects(
                            ObjectsMsg::Message(ObjectsMessage::MoveDown),
                        )))
                    };
                    let result = process_message_round(effect_runner, action_rx, msg, state);
                    dirty |= result.dirty;
                }
            }
            // Scroll wheel: route to explorer instances pane when
            // focus is on Explorer Instances and mouse is inside it.
            MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
                if matches!(
                    state.focus,
                    Pane::Explorer(crate::app_shell::nav::ExplorerPane::Instances)
                ) =>
            {
                // Shift+wheel is the platform convention for
                // horizontal scrolling; some terminals instead
                // report the gesture natively as ScrollLeft/Right.
                let (horizontal, dir) = wheel_axis(mouse.kind, mouse.modifiers);
                let now = std::time::Instant::now();
                if let Some((t, d, h)) = last_wheel
                    && d == dir
                    && h == horizontal
                    && now.duration_since(t).as_millis() < WHEEL_DEBOUNCE_MS
                {
                    idle_iterations = 0;
                    skip_iteration = true;
                    break 'mouse;
                }
                last_wheel = Some((now, dir, horizontal));

                let size = terminal.size()?;
                let footer_h = footer_view::footer_height(&state.footer, size.width);
                let body_top = 3u16;
                let body_h = size
                    .height
                    .saturating_sub(body_top)
                    .saturating_sub(footer_h);
                let point = ratatui::layout::Position::new(mouse.column, mouse.row);
                if let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
                    && let (inst_area, _objs_area) =
                        explorer_child_areas(explorer, state.explorer.splitter.instances_height)
                    && inst_area.contains(point)
                {
                    use crate::features::explorer::instances::msg::{
                        InstancesMessage, InstancesMsg,
                    };
                    use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
                    // Shift+wheel scrolls the tree sideways; a bare
                    // wheel keeps moving the cursor up/down.
                    let msg = if horizontal {
                        AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
                            InstancesMsg::Message(InstancesMessage::ScrollHorizontal {
                                delta: (dir * WHEEL_H_STEP) as i16,
                                term_width: size.width,
                            }),
                        )))
                    } else if dir < 0 {
                        AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
                            InstancesMsg::Message(InstancesMessage::MoveUp),
                        )))
                    } else {
                        AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
                            InstancesMsg::Message(InstancesMessage::MoveDown),
                        )))
                    };
                    let result = process_message_round(effect_runner, action_rx, msg, state);
                    dirty |= result.dirty;
                }
            }
            // Scroll wheel: route to IW connections when focus is on
            // InstanceWorkspace Connections AND mouse is inside IW body.
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if matches!(
                    state.focus,
                    Pane::InstanceWorkspace(crate::app_shell::nav::IwPane::Connections)
                ) =>
            {
                // Shift+wheel is the platform convention for
                // horizontal scrolling; some terminals instead
                // report the gesture natively as ScrollLeft/Right.
                let (horizontal, dir) = wheel_axis(mouse.kind, mouse.modifiers);
                let now = std::time::Instant::now();
                if let Some((t, d, h)) = last_wheel
                    && d == dir
                    && h == horizontal
                    && now.duration_since(t).as_millis() < WHEEL_DEBOUNCE_MS
                {
                    idle_iterations = 0;
                    skip_iteration = true;
                    break 'mouse;
                }
                last_wheel = Some((now, dir, horizontal));

                let size = terminal.size()?;
                if let Some(body) = iw_body_area_for_hit(size, state)
                    && let point = ratatui::layout::Position::new(mouse.column, mouse.row)
                    && body.contains(point)
                {
                    use crate::features::instance_workspace::connections::msg::{
                        ConnectionsMessage, ConnectionsMsg,
                    };
                    use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
                    // No horizontal scroll in this pane, so a
                    // shift-wheel falls through to the vertical axis.
                    let msg = match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
                                ConnectionsMsg::Message(ConnectionsMessage::MoveUp),
                            )))
                        }
                        MouseEventKind::ScrollDown => {
                            AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
                                ConnectionsMsg::Message(ConnectionsMessage::MoveDown),
                            )))
                        }
                        _ => unreachable!(),
                    };
                    let result = process_message_round(effect_runner, action_rx, msg, state);
                    dirty |= result.dirty;
                }
            }
            // Scroll wheel: route to IW overview when focus is on
            // InstanceWorkspace Overview AND mouse is inside IW body.
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if matches!(
                    state.focus,
                    Pane::InstanceWorkspace(crate::app_shell::nav::IwPane::Overview)
                ) =>
            {
                // Shift+wheel is the platform convention for
                // horizontal scrolling; some terminals instead
                // report the gesture natively as ScrollLeft/Right.
                let (horizontal, dir) = wheel_axis(mouse.kind, mouse.modifiers);
                let now = std::time::Instant::now();
                if let Some((t, d, h)) = last_wheel
                    && d == dir
                    && h == horizontal
                    && now.duration_since(t).as_millis() < WHEEL_DEBOUNCE_MS
                {
                    idle_iterations = 0;
                    skip_iteration = true;
                    break 'mouse;
                }
                last_wheel = Some((now, dir, horizontal));

                let size = terminal.size()?;
                if let Some(body) = iw_body_area_for_hit(size, state)
                    && let point = ratatui::layout::Position::new(mouse.column, mouse.row)
                    && body.contains(point)
                {
                    use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
                    use crate::features::instance_workspace::overview::msg::{
                        OverviewMessage, OverviewMsg,
                    };
                    // No horizontal scroll in this pane, so a
                    // shift-wheel falls through to the vertical axis.
                    let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Overview(
                        OverviewMsg::Message(OverviewMessage::MoveCursor(dir)),
                    )));
                    let result = process_message_round(effect_runner, action_rx, msg, state);
                    dirty |= result.dirty;
                }
            }
            // Scroll wheel: route to the history list when the
            // mouse is inside it, matching discover targets'
            // focus+position gating. Moving the cursor pushes the
            // viewport in discover-style mode (cursor at top of
            // viewport → can scroll up; cursor at bottom → can
            // scroll down).
            MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
                if matches!(state.focus, Pane::SQLWorkspace) =>
            {
                // Wheel debounce — same as discover targets above.
                // Shift+wheel is the platform convention for
                // horizontal scrolling; some terminals instead
                // report the gesture natively as ScrollLeft/Right.
                let (horizontal, dir) = wheel_axis(mouse.kind, mouse.modifiers);
                let now = std::time::Instant::now();
                if let Some((t, d, h)) = last_wheel
                    && d == dir
                    && h == horizontal
                    && now.duration_since(t).as_millis() < WHEEL_DEBOUNCE_MS
                {
                    idle_iterations = 0;
                    skip_iteration = true;
                    break 'mouse;
                }
                last_wheel = Some((now, dir, horizontal));

                if let Some(size) = terminal.size().ok()
                    && let Some(tab_area) = sql_tab_area_for_hit(size, state)
                    && let Some(tab_idx) = state.sql.sql_tab.active_tab
                    && let Some(tab) = state.sql.sql_tab.tabs.get(tab_idx)
                {
                    use crate::features::sql_workspace::sql_tab::layout::sql_tab_layout;
                    let layout = sql_tab_layout(
                        tab_area,
                        tab.splitter.editor_top_height,
                        tab.splitter.history_pane_width,
                    );
                    // Route wheel to the editor body first (top-left
                    // zone), then to history (right/bottom zone).
                    // The two panes do not overlap.
                    if layout.editor.contains(point) {
                        use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
                        use crate::features::sql_workspace::sql_tab::editor::msg::{
                            EditorMessage, EditorMsg,
                        };
                        use crate::features::sql_workspace::sql_tab::msg::{
                            SqlTabMessage, SqlTabMsg,
                        };
                        // Shift+wheel scrolls the editor sideways;
                        // a bare wheel keeps scrolling by lines.
                        let delta: i32 = if horizontal {
                            dir * WHEEL_H_STEP
                        } else {
                            dir * 3
                        };
                        let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                            SqlTabMsg::Message(SqlTabMessage::Editor {
                                tab_id: tab_idx,
                                msg: EditorMsg::Message(if horizontal {
                                    EditorMessage::ScrollH { delta }
                                } else {
                                    EditorMessage::ScrollV { delta }
                                }),
                            }),
                        )));
                        let result = process_message_round(effect_runner, action_rx, msg, state);
                        dirty |= result.dirty;
                    } else {
                        // When detail is visible the History zone extends
                        // leftward into the editor. For wheel routing we
                        // compute the full zone rect (detail + list +
                        // splitter) so wheel works anywhere inside it.
                        use crate::features::sql_workspace::sql_tab::session::session_view_key;
                        let (instance, connection) = session_view_key(&tab.session);
                        let detail_visible = crate::features::sql_workspace::sql_tab::history::detail_visible(
                                        tab.focus == crate::features::sql_workspace::sql_tab::state::SqlFocus::History,
                                        &tab.history.list,
                                        &state.sql.sql_tab.history_store,
                                        &instance,
                                        &connection,
                                    );
                        let zone_rect = if detail_visible {
                            use crate::features::sql_workspace::sql_tab::history::splitter::view::{history_zone_x, history_zone_width};
                            let zone_x = history_zone_x(
                                tab_area,
                                &layout,
                                tab.history.splitter.detail_pane_width,
                            );
                            let zone_w =
                                history_zone_width(&layout, tab.history.splitter.detail_pane_width);
                            ratatui::layout::Rect::new(
                                zone_x,
                                layout.history.y,
                                zone_w,
                                layout.history.height,
                            )
                        } else {
                            layout.history
                        };
                        if zone_rect.contains(point) {
                            use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
                            use crate::features::sql_workspace::sql_tab::history::msg::{
                                HistoryMessage, HistoryMsg,
                            };
                            use crate::features::sql_workspace::sql_tab::msg::{
                                SqlTabMessage, SqlTabMsg,
                            };
                            // Shift+wheel scrolls the history rows
                            // sideways; a bare wheel moves the cursor.
                            let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                                SqlTabMsg::Message(SqlTabMessage::History {
                                    tab_id: tab_idx,
                                    msg: HistoryMsg::Message(if horizontal {
                                        HistoryMessage::ScrollHScroll {
                                            delta: dir * WHEEL_H_STEP,
                                        }
                                    } else {
                                        HistoryMessage::MoveCursor { delta: dir }
                                    }),
                                }),
                            )));
                            let result =
                                process_message_round(effect_runner, action_rx, msg, state);
                            dirty |= result.dirty;
                        } else if layout.results.contains(point) {
                            // Scroll wheel on the results pane: move
                            // the cell selection up/down. The view
                            // auto-adjusts v_scroll to keep cursor visible.
                            use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
                            use crate::features::sql_workspace::sql_tab::msg::{
                                SqlTabMessage, SqlTabMsg,
                            };
                            use crate::features::sql_workspace::sql_tab::results::msg::{
                                ResultsMessage, ResultsMsg,
                            };
                            // Shift+wheel scrolls the grid sideways;
                            // a bare wheel moves the cell selection.
                            let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                                SqlTabMsg::Message(SqlTabMessage::Results {
                                    tab_id: tab_idx,
                                    msg: ResultsMsg::Message(if horizontal {
                                        ResultsMessage::ScrollHScroll {
                                            delta: dir * WHEEL_H_STEP,
                                        }
                                    } else {
                                        ResultsMessage::MoveSelection { dr: dir, dc: 0 }
                                    }),
                                }),
                            )));
                            let result =
                                process_message_round(effect_runner, action_rx, msg, state);
                            dirty |= result.dirty;
                        }
                    }
                }
            }
            _ => {
                tracing::trace!(
                    is_left_down = matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)),
                    modal_open = state.modal.is_some(),
                    "mouse event ignored"
                );
            }
        }
    }

    // Hand the gesture state back: the run loop owns it so it survives into the
    // next event without ever entering `AppState`.
    interaction.splitter_drag = splitter_drag;
    interaction.last_click = last_click;
    interaction.last_wheel = last_wheel;
    interaction.idle_iterations = idle_iterations;

    Ok(if skip_iteration {
        MouseOutcome::Continue
    } else {
        MouseOutcome::Handled { repaint: dirty }
    })
}
/// A left press while a confirm modal is open: hit-test the Yes/No
/// buttons against the rendered popup.
fn handle_confirm_modal_click(
    terminal: &mut AppTerminal,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    point: Position,
    dirty: &mut bool,
) -> anyhow::Result<()> {
    let size = terminal.size()?;
    let footer_h = footer_view::footer_height(&state.footer, size.width);
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    // The confirm modal renders over the live workspace
    // (app_body_layout), so hit-test against the same.
    if let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state) {
        let popup = crate::common::view::modal::confirm_popup_rect(
            workspace,
            crate::common::view::modal::confirm_body_rows(state.modal.as_ref().unwrap()),
        );
        let buttons = crate::common::view::modal::confirm_buttons(popup);
        let msg = if buttons.yes_rect.contains(point) {
            crate::app::input::confirm_yes_msg(state.modal.as_ref().unwrap(), state)
        } else if buttons.no_rect.contains(point) {
            Some(AppMsg::CloseModal)
        } else {
            None
        };
        if let Some(msg) = msg {
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }
    Ok(())
}

/// A left press on discover's close-confirmation dialog.
fn handle_discover_close_confirm_click(
    terminal: &mut AppTerminal,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    point: Position,
    dirty: &mut bool,
) -> anyhow::Result<()> {
    let size = terminal.size()?;
    let footer_h = footer_view::footer_height(&state.footer, size.width);
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    // The workspace region matches the render exactly
    // (using the live Explorer splitter width, not a
    // hard-coded 20%), and the close-confirm popup is
    // centered inside the discover overlay (75% of the
    // workspace), matching `render_modal_popup` — so the
    // popup the mouse hits is the same one that is drawn.
    let msg = if let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state) {
        // The close-confirm popup is centered inside the
        // discover overlay (75% of the workspace), matching
        // `render_modal_popup`, so the popup the mouse hits
        // is the same one that is drawn.
        let discover_popup = crate::common::view::modal::popup_rect(workspace, 75, 75);
        // Discover's close-confirm body is a single line.
        let popup = crate::common::view::modal::confirm_popup_rect(discover_popup, 1);
        let buttons = crate::common::view::modal::confirm_buttons(popup);
        if buttons.yes_rect.contains(point) {
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
        }
    } else {
        None
    };
    if let Some(msg) = msg {
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }
    Ok(())
}

/// A left press on the app body: focus the clicked pane, grab a scrollbar
/// or splitter, or dispatch the pane's own click action.
// Four of these are the shared dispatch plumbing (terminal, state, the message
// round) and three are the gesture outputs it owns. Bundling them into a
// context struct is the natural follow-up once the click routing is split out
// further; a flat signature keeps the moved body readable until then.
#[allow(clippy::too_many_arguments)]
fn handle_down(
    mouse: &MouseEvent,
    terminal: &mut AppTerminal,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    point: Position,
    dirty: &mut bool,
    splitter_drag: &mut Option<SplitterDrag>,
    last_click: &mut Option<(Position, Instant)>,
) -> anyhow::Result<()> {
    // Double-click detection: a second press at the same
    // cell within the window is treated as a double click.
    let is_double_click = last_click
        .as_ref()
        .is_some_and(|(p, t)| *p == point && t.elapsed() < std::time::Duration::from_millis(400));
    *last_click = Some((point, std::time::Instant::now()));

    // Map the click to a focus pane by region. The layout
    // mirrors `app/view.rs`: header (top 3 rows), explorer
    // (left 20% of the body), workspace (right 80%).
    let size = terminal.size()?;
    let footer_h = footer_view::footer_height(&state.footer, size.width);
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    // Use the live Explorer column width (not a hard-coded
    // 20%) so clicks/hits inside the resizable Explorer
    // agree with the rendered splitter.
    let explorer_w = app_explorer_rect(size, body_top, body_h, state)
        .map(|r| r.width)
        .unwrap_or((size.width.saturating_mul(2) / 10).max(1));
    // Starting a drag on the discover targets/results
    // splitter (only while discover owns focus) begins a
    // resize gesture. It is checked before sub-pane
    // switching below. The body uses the same live
    // workspace/popup/engine geometry the render uses.
    if matches!(state.focus, Pane::Discover(_))
        && !state.discover.close_confirm
        && let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state)
        && let discover_popup = crate::common::view::modal::popup_rect(workspace, 75, 75)
        && let body =
            crate::features::discover::view::discover_body_area(discover_popup, &state.discover)
        && body.height >= 3
        && let layout = crate::features::discover::splitter::view::discover_body_layout(
            body,
            state.discover.splitter.targets_height,
        )
        && crate::features::discover::splitter::view::splitter_at(&layout, point.x, point.y)
    {
        *splitter_drag = Some(SplitterDrag::Discover);
        state.splitter_hover.discover_splitter_drag = true;
        tracing::debug!("discover targets/results splitter drag started");
    }

    // A click outside the open context picker closes it
    // (mirroring the original dbm), regardless of which
    // pane the click lands in. The SQL workspace click
    // handler also closes for clicks in its body, so this
    // only fires for clicks outside the picker area.
    if let Some(picker_area) = sql_picker_area_for_hit(size, state)
        && !picker_area.contains(point)
    {
        let msg = sql_click_msgs(
            &state.sql.sql_tab,
            crate::features::sql_workspace::sql_tab::view::SqlClickAction::CloseContextPicker,
        );
        for m in msg {
            let result = process_message_round(effect_runner, action_rx, m, state);
            *dirty |= result.dirty;
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
        // Exception: clicks on the explorer instances/
        // objects splitter keep the current sub-pane —
        // the splitter is a drag handle, not a clickable
        // region.
        let current_sub = match state.focus {
            Pane::Explorer(sub) => Some(sub),
            _ => None,
        };
        let hit_splitter = if let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
            && explorer.height >= 3
        {
            let inner = Rect::new(
                explorer.x.saturating_add(1),
                explorer.y.saturating_add(1),
                explorer.width.saturating_sub(2),
                explorer.height.saturating_sub(2),
            );
            let layout = crate::features::explorer::splitter::view::explorer_body_layout(
                inner,
                state.explorer.splitter.instances_height,
            );
            crate::features::explorer::splitter::view::splitter_at(&layout, mouse.column, mouse.row)
        } else {
            false
        };
        if hit_splitter && let Some(sub) = current_sub {
            Some(Pane::Explorer(sub))
        } else {
            Some(Pane::Explorer(explorer_pane_for_click(
                mouse.row,
                body_top,
                body_h,
                state.explorer.splitter.instances_height,
            )))
        }
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
            crate::features::instance_workspace::view::iw_tab_at(ws, mouse.column, mouse.row)
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
        let msg = AppMsg::Shell(crate::app_shell::msg::ShellMsg::FocusChanged { pane });
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }

    // A single click inside the explorer's instances or
    // objects tree moves the selection (cursor) to the
    // clicked row — UNLESS the click lands on a v_scrollbar
    // or h_scrollbar track, which must NOT also move the
    // cursor to that row.
    let click_hits_explorer_scrollbar = {
        let mut hits = false;
        if let Some(explorer) = app_explorer_rect(size, body_top, body_h, state) {
            let (inst_area, objs_area) =
                explorer_child_areas(explorer, state.explorer.splitter.instances_height);
            use crate::features::explorer::instances as inst_mod;
            use crate::features::explorer::objects as obj_mod;
            if inst_area.contains(point) {
                hits |= inst_mod::view::v_scrollbar_hit(
                    inst_area,
                    &state.explorer.instances,
                    mouse.column,
                    mouse.row,
                )
                .is_some();
                hits |= inst_mod::view::h_scrollbar_hit(
                    inst_area,
                    &state.explorer.instances,
                    mouse.column,
                    mouse.row,
                )
                .is_some();
            }
            if objs_area.contains(point) {
                hits |= obj_mod::view::v_scrollbar_hit(
                    objs_area,
                    &state.explorer.objects,
                    mouse.column,
                    mouse.row,
                )
                .is_some();
                hits |= obj_mod::view::h_scrollbar_hit(
                    objs_area,
                    &state.explorer.objects,
                    mouse.column,
                    mouse.row,
                )
                .is_some();
            }
        }
        hits
    };
    if mouse.column < explorer_w
        && !click_hits_explorer_scrollbar
        && let Some(click_msgs) =
            explorer_row_click_msgs(size, body_top, body_h, mouse.column, mouse.row, state)
    {
        for click_msg in click_msgs {
            let result = process_message_round(effect_runner, action_rx, click_msg, state);
            *dirty |= result.dirty;
        }
    }

    // A double click in the explorer activates the node
    // (Select), like pressing Enter on it — except on an
    // expand/collapse marker (toggle only) or on blank
    // space (no node: do nothing, not act on the cursor).
    if is_double_click
        && mouse.column < explorer_w
        && !click_hits_explorer_scrollbar
        && explorer_click_hits_row(explorer_w, body_top, body_h, mouse.row, state)
        && !is_explorer_toggle_click(explorer_w, body_top, body_h, mouse.column, mouse.row, state)
    {
        let select = AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
            match explorer_pane_for_click(
                mouse.row,
                body_top,
                body_h,
                state.explorer.splitter.instances_height,
            ) {
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
        ));
        let result = process_message_round(effect_runner, action_rx, select, state);
        *dirty |= result.dirty;
    }

    // A click inside the SQL workspace also routes to a
    // sub-pane (editor/history/results) or activates the
    // clicked tab, mirroring the mouse support of the
    // original dbm.
    if matches!(target_pane, Some(Pane::SQLWorkspace))
        && let Some(tab_area) = sql_tab_area_for_hit(size, state)
        && let Some(action) = crate::features::sql_workspace::sql_tab::view::sql_workspace_click(
            &state.sql.sql_tab,
            tab_area,
            mouse.column,
            mouse.row,
            is_double_click,
        )
    {
        // If the click was on the h_scrollbar, start dragging.
        if let crate::features::sql_workspace::sql_tab::view::SqlClickAction::HistoryHScrollbar {
            track_x,
            x: _,
            max_scroll,
            viewport_width,
        } = action
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::HistoryH,
                track_start: track_x,
                viewport_len: viewport_width,
                max_scroll,
            });
        }
        // If the click was on the v_scrollbar, start dragging.
        if let crate::features::sql_workspace::sql_tab::view::SqlClickAction::HistoryVScrollbar {
            track_y,
            y: _,
            max_scroll,
            viewport_height,
        } = action
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::HistoryV,
                track_start: track_y,
                viewport_len: viewport_height,
                max_scroll,
            });
        }
        // If the click was on the editor body's v_scrollbar, start dragging.
        if let crate::features::sql_workspace::sql_tab::view::SqlClickAction::EditorVScrollbar {
            track_y,
            y: _,
            max_scroll,
            viewport_height,
        } = action
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::SqlV,
                track_start: track_y,
                viewport_len: viewport_height,
                max_scroll,
            });
        }
        // If the click was on the results list h_scrollbar, start dragging.
        if let crate::features::sql_workspace::sql_tab::view::SqlClickAction::ResultsHScrollbar {
            track_x,
            x: _,
            max_scroll,
            viewport_width,
        } = action
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::ResultsH,
                track_start: track_x,
                viewport_len: viewport_width,
                max_scroll,
            });
        }
        // If the click was on the results list v_scrollbar, start dragging.
        if let crate::features::sql_workspace::sql_tab::view::SqlClickAction::ResultsVScrollbar {
            track_y,
            y: _,
            max_scroll,
            viewport_height,
        } = action
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::ResultsV,
                track_start: track_y,
                viewport_len: viewport_height,
                max_scroll,
            });
        }
        // A single click on a results column header
        // splitter begins a column-width resize drag.
        if let crate::features::sql_workspace::sql_tab::view::SqlClickAction::ResultsColResize {
            col,
        } = action
        {
            state.splitter_hover.results_col_resize_drag = Some(col);
            tracing::debug!(col, "results column resize drag started");
        }
        for msg in sql_click_msgs(&state.sql.sql_tab, action) {
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // Inside the discover popup: map the click's row to a
    // discover child pane and switch focus to it. This is
    // suppressed while the close-confirmation dialog is
    // open, so clicks on the discover pane behind it are
    // ignored (only Enter/Esc operate on the dialog).
    if let Pane::Discover(sub) = state.focus
        && !state.discover.close_confirm
        && splitter_drag.is_none()
        && let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state)
        && let Some(next) =
            discover_subpane_for_click(mouse.column, mouse.row, workspace, &state.discover)
        && next != sub
    {
        let msg = AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
            crate::features::discover::msg::DiscoverMessage::Focus(next),
        ));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
        tracing::debug!(to = ?next, "mouse click switched discover sub-pane");
    }

    // Inside the discover popup's targets pane: row/cell
    // selection on single click, begin-edit on double
    // click (matching original dbm behavior).
    if let Pane::Discover(sub) = state.focus
        && !state.discover.close_confirm
        && splitter_drag.is_none()
        && matches!(sub, crate::app_shell::nav::DiscoverPane::Targets)
        && let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state)
        && let discover_popup = crate::common::view::modal::popup_rect(workspace, 75, 75)
        && let body =
            crate::features::discover::view::discover_body_area(discover_popup, &state.discover)
        && !body.is_empty()
        && let layout = crate::features::discover::splitter::view::discover_body_layout(
            body,
            state.discover.splitter.targets_height,
        )
        && layout.targets.contains(point)
    {
        use crate::features::discover::targets::{
            msg::{TargetsMessage, TargetsMsg},
            view,
        };
        // 1. Scrollbar hit-test first — scrollbar sits
        //    outside content area so hit_test misses it.
        //    If we hit the v_scrollbar, start a drag.
        if let Some(si) = view::v_scrollbar_hit(
            layout.targets,
            &state.discover.targets,
            mouse.column,
            mouse.row,
        ) {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::DiscoverTargetsV,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::view::pane_scrollbar::scroll_offset_from_track(
                mouse.row,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg =
                crate::app::AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                    crate::features::discover::msg::DiscoverMessage::Targets(TargetsMsg::Message(
                        TargetsMessage::SetVScroll {
                            position: new_scroll,
                        },
                    )),
                ));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
        // 2. Content-area click: select row / cell.
        else if let Some((row, col)) = view::hit_test(
            layout.targets,
            &state.discover.targets,
            mouse.column,
            mouse.row,
        ) {
            let msg = if is_double_click {
                if let Some(col) = col {
                    AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                        crate::features::discover::msg::DiscoverMessage::Targets(
                            TargetsMsg::Message(TargetsMessage::BeginEditCell { row, col }),
                        ),
                    ))
                } else {
                    AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                        crate::features::discover::msg::DiscoverMessage::Targets(
                            TargetsMsg::Message(TargetsMessage::SelectRow { row }),
                        ),
                    ))
                }
            } else if let Some(col) = col {
                AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                    crate::features::discover::msg::DiscoverMessage::Targets(TargetsMsg::Message(
                        TargetsMessage::SelectCell { row, col },
                    )),
                ))
            } else {
                AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                    crate::features::discover::msg::DiscoverMessage::Targets(TargetsMsg::Message(
                        TargetsMessage::SelectRow { row },
                    )),
                ))
            };
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // Discover results pane click handler — v_scrollbar
    // hit-test first, then content row click (select +
    // toggle). Much simpler than targets (no cells).
    if let Pane::Discover(sub) = state.focus
        && !state.discover.close_confirm
        && splitter_drag.is_none()
        && matches!(sub, crate::app_shell::nav::DiscoverPane::Results)
        && let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state)
        && let discover_popup = crate::common::view::modal::popup_rect(workspace, 75, 75)
        && let body =
            crate::features::discover::view::discover_body_area(discover_popup, &state.discover)
        && !body.is_empty()
        && let layout = crate::features::discover::splitter::view::discover_body_layout(
            body,
            state.discover.splitter.targets_height,
        )
        && layout.results.contains(point)
    {
        use crate::features::discover::msg::{DiscoverMessage, DiscoverMsg};
        use crate::features::discover::results::{
            msg::{ResultsMessage, ResultsMsg},
            view,
        };
        // 1. Scrollbar hit-test first.
        if let Some(si) = view::v_scrollbar_hit(
            layout.results,
            &state.discover.results,
            mouse.column,
            mouse.row,
        ) {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::DiscoverResultsV,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::view::pane_scrollbar::scroll_offset_from_track(
                mouse.row,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg = AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Results(
                ResultsMsg::Message(ResultsMessage::SetVScroll {
                    position: new_scroll,
                }),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
        // 2. Content-area click: select row. A single
        //    click moves cursor + toggles selection (the
        //    discover results pane's primary interaction).
        else if let Some(row) = view::hit_test(
            layout.results,
            &state.discover.results,
            mouse.column,
            mouse.row,
        ) {
            // Move cursor to clicked row first (also
            // clears scroll_locked), then toggle selection.
            let focus_msg = AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Results(
                ResultsMsg::Message(ResultsMessage::SetCursor { row }),
            )));
            let r1 = process_message_round(effect_runner, action_rx, focus_msg, state);
            *dirty |= r1.dirty;
            let toggle_msg = AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Results(
                ResultsMsg::Message(ResultsMessage::ToggleSelect),
            )));
            let r2 = process_message_round(effect_runner, action_rx, toggle_msg, state);
            *dirty |= r2.dirty;
        }
    }

    // Explorer objects v_scrollbar hit-test — runs BEFORE
    // the generic explorer_row_click_msgs jump so a
    // scrollbar click starts a drag instead of moving
    // the cursor to that row.
    if let Pane::Explorer(_) = state.focus
        && let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
        && let (_inst_area, objs_area) =
            explorer_child_areas(explorer, state.explorer.splitter.instances_height)
        && objs_area.contains(point)
    {
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        use crate::features::explorer::objects::{msg::*, view};
        if let Some(si) =
            view::v_scrollbar_hit(objs_area, &state.explorer.objects, mouse.column, mouse.row)
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::ObjectsV,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::view::pane_scrollbar::scroll_offset_from_track(
                mouse.row,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Objects(
                ObjectsMsg::Message(ObjectsMessage::SetVScroll {
                    position: new_scroll,
                }),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // Explorer instances v_scrollbar hit-test — before
    // explorer_row_click_msgs so a scrollbar click
    // starts a drag instead of moving the cursor.
    if let Pane::Explorer(_) = state.focus
        && let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
        && let (inst_area, _objs_area) =
            explorer_child_areas(explorer, state.explorer.splitter.instances_height)
        && inst_area.contains(point)
    {
        use crate::features::explorer::instances::{msg::*, view};
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        if let Some(si) = view::v_scrollbar_hit(
            inst_area,
            &state.explorer.instances,
            mouse.column,
            mouse.row,
        ) {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::TreeV,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::view::pane_scrollbar::scroll_offset_from_track(
                mouse.row,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
                InstancesMsg::Message(InstancesMessage::SetVScroll {
                    position: new_scroll,
                }),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // Explorer objects h_scrollbar hit-test.
    if let Pane::Explorer(_) = state.focus
        && let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
        && let (_inst_area, objs_area) =
            explorer_child_areas(explorer, state.explorer.splitter.instances_height)
        && objs_area.contains(point)
    {
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        use crate::features::explorer::objects::{msg::*, view};
        if let Some(si) =
            view::h_scrollbar_hit(objs_area, &state.explorer.objects, mouse.column, mouse.row)
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::ObjectsH,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::view::pane_scrollbar::scroll_offset_from_track(
                mouse.column,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Objects(
                ObjectsMsg::Message(ObjectsMessage::SetHScroll {
                    position: new_scroll,
                }),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // Explorer instances h_scrollbar hit-test.
    if let Pane::Explorer(_) = state.focus
        && let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
        && let (inst_area, _objs_area) =
            explorer_child_areas(explorer, state.explorer.splitter.instances_height)
        && inst_area.contains(point)
    {
        use crate::features::explorer::instances::{msg::*, view};
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        if let Some(si) = view::h_scrollbar_hit(
            inst_area,
            &state.explorer.instances,
            mouse.column,
            mouse.row,
        ) {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::TreeH,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::view::pane_scrollbar::scroll_offset_from_track(
                mouse.column,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
                InstancesMsg::Message(InstancesMessage::SetHScroll {
                    position: new_scroll,
                }),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // IW connections v_scrollbar hit-test.
    if let Some(body) = iw_body_area_for_hit(size, state)
        && body.contains(point)
        && matches!(state.iw.pane, crate::app_shell::nav::IwPane::Connections)
    {
        use crate::features::instance_workspace::connections::{msg::*, view};
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        if let Some(si) =
            view::v_scrollbar_hit(body, &state.iw.connections, mouse.column, mouse.row)
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::ConnectionsV,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::view::pane_scrollbar::scroll_offset_from_track(
                mouse.row,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
                ConnectionsMsg::Message(ConnectionsMessage::SetVScroll {
                    position: new_scroll,
                }),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // IW connections list click: a single click moves the
    // cursor to the clicked row; a double click opens the
    // edit form for it. Excludes clicks on the v_scrollbar
    // track (which start a drag instead) and clicks while
    // the add/edit form is open (the popup owns the input).
    if let Some(body) = iw_body_area_for_hit(size, state)
        && body.contains(point)
        && matches!(state.iw.pane, crate::app_shell::nav::IwPane::Connections)
        && state.iw.connections.form.is_none()
    {
        use crate::features::instance_workspace::connections::{msg::*, view};
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        let on_scrollbar =
            view::v_scrollbar_hit(body, &state.iw.connections, mouse.column, mouse.row).is_some();
        if !on_scrollbar && let Some(row) = view::row_at(body, &state.iw.connections, mouse.row) {
            let msg = if is_double_click {
                // Double click edits the connection, like
                // pressing `i` on the row (BeginEdit).
                ConnectionsMessage::BeginEdit
            } else {
                // Single click selects the row (JumpTo).
                ConnectionsMessage::JumpTo { row }
            };
            let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
                ConnectionsMsg::Message(msg),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // IW connections form click: when the add/edit form
    // popup is open, a single click on a field line
    // selects that field and a double click enters insert
    // mode on it (matching the original dbm's form field
    // click handling). Clicks outside a field line are
    // ignored.
    if let Some(body) = iw_body_area_for_hit(size, state)
        && body.contains(point)
        && matches!(state.iw.pane, crate::app_shell::nav::IwPane::Connections)
        && state.iw.connections.form.is_some()
    {
        use crate::features::instance_workspace::connections::{msg::*, view};
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        if let Some(field) = view::form_field_at(body, mouse.column, mouse.row) {
            let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
                ConnectionsMsg::Message(ConnectionsMessage::FormClick {
                    field,
                    is_double: is_double_click,
                }),
            )));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // IW overview v_scrollbar hit-test.
    if let Some(body) = iw_body_area_for_hit(size, state)
        && body.contains(point)
        && matches!(state.iw.pane, crate::app_shell::nav::IwPane::Overview)
    {
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        use crate::features::instance_workspace::overview::{msg::*, view};
        // overview_rows conn_count parameter for viewport calc;
        // 0 is fine for geometry (only affects row content).
        if let Some(si) =
            view::v_scrollbar_hit(body, &state.iw.overview, 0, mouse.column, mouse.row)
        {
            state.scrollbar_drag = Some(ScrollbarDrag {
                which: ActiveScrollbar::OverviewV,
                track_start: si.track_start,
                viewport_len: si.track_len,
                max_scroll: si.max_scroll,
            });
            let new_scroll = crate::common::view::pane_scrollbar::scroll_offset_from_track(
                mouse.row,
                si.track_start,
                si.track_len,
                si.max_scroll,
            );
            let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Overview(OverviewMsg::Message(
                OverviewMessage::SetVScroll {
                    position: new_scroll,
                },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // IW overview: clicking a row moves the overview
    // cursor to it (like the connections list's single
    // click). The overview has no double-click action.
    if let Some(body) = iw_body_area_for_hit(size, state)
        && body.contains(point)
        && matches!(state.iw.pane, crate::app_shell::nav::IwPane::Overview)
    {
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        use crate::features::instance_workspace::overview::{msg::*, view};
        let on_scrollbar =
            view::v_scrollbar_hit(body, &state.iw.overview, 0, mouse.column, mouse.row).is_some();
        if !on_scrollbar && let Some(row) = view::row_at(body, &state.iw.overview, 0, mouse.row) {
            let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Overview(OverviewMsg::Message(
                OverviewMessage::SetCursor { index: row },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // Left-click on the header `Discover` button activates
    // it, in addition to moving focus to the header.
    let header_area = Rect::new(0, 0, size.width, 3);
    let button_rect = crate::features::header::view::discover_button_rect(header_area);
    let clicked = button_rect.is_some_and(|r| r.contains(point));
    tracing::debug!(clicked, "header button click resolved");
    if clicked {
        // Clicking the header button is an explicit user
        // intent: move focus to the Header pane (via the
        // shell message), then dispatch Activate. Both go
        // through `update` so every state change flows
        // through the single state-transition channel.
        let result = process_message_round(
            effect_runner,
            action_rx,
            AppMsg::Shell(crate::app_shell::msg::ShellMsg::FocusChanged { pane: Pane::Header }),
            state,
        );
        *dirty |= result.dirty;
        let result = process_message_round(
            effect_runner,
            action_rx,
            AppMsg::Header(HeaderMsg::Message(HeaderMessage::Activate)),
            state,
        );
        *dirty |= result.dirty;
        tracing::debug!("dispatching HeaderMessage::Activate");
    }

    // Resolve the press to the *single* splitter being
    // dragged (see `resolve_splitter_drag`). The
    // discover splitter is resolved earlier — before the
    // click re-maps focus — and already holds the slot
    // when it hit, which is why this is skipped then.
    if splitter_drag.is_none()
        && let Some(target) = resolve_splitter_drag(state, size, point.x, point.y)
    {
        *splitter_drag = Some(target);
        use crate::features::sql_workspace::sql_tab::splitter::view::SqlSplitter;
        let sh = &mut state.splitter_hover;
        match target {
            SplitterDrag::App => sh.app_splitter_drag = true,
            SplitterDrag::Explorer => sh.explorer_splitter_drag = true,
            SplitterDrag::Discover => sh.discover_splitter_drag = true,
            SplitterDrag::Sql(_, SqlSplitter::EditorResults) => sh.sql_editor_results_drag = true,
            SplitterDrag::Sql(_, SqlSplitter::EditorHistory) => sh.sql_editor_history_drag = true,
            SplitterDrag::Sql(_, SqlSplitter::HistoryDetail) => sh.sql_history_detail_drag = true,
            SplitterDrag::Sql(_, SqlSplitter::ResultsDetail) => sh.sql_results_detail_drag = true,
        }
        tracing::debug!(?target, "splitter drag started");
    }
    Ok(())
}

/// A held-button drag: follow the pointer for whichever scrollbar or
/// splitter the press armed.
fn handle_drag(
    terminal: &mut AppTerminal,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    point: Position,
    dirty: &mut bool,
    splitter_drag: &mut Option<SplitterDrag>,
) -> anyhow::Result<()> {
    // Exactly one of the arms below can match:
    // `splitter_drag` is a single slot armed by the
    // press, so a drag resizes only the splitter it
    // started on — and only along that splitter's own
    // axis (a vertical splitter reads `x`, a horizontal
    // one reads `y`).
    if *splitter_drag == Some(SplitterDrag::Discover) {
        let size = terminal.size()?;
        let body_top = 3u16;
        let body_h = size
            .height
            .saturating_sub(body_top)
            .saturating_sub(footer_view::footer_height(&state.footer, size.width));
        // Use the same workspace/popup/body geometry the
        // render uses (live Explorer width + dynamic
        // engine height), so the drag track matches the
        // rendered split exactly.
        if let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state) {
            let discover_popup = crate::common::view::modal::popup_rect(workspace, 75, 75);
            let body = crate::features::discover::view::discover_body_area(
                discover_popup,
                &state.discover,
            );
            if body.height >= 3 {
                let height =
                    crate::features::discover::splitter::view::targets_height_for_y(body, point.y);
                let msg = AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                    crate::features::discover::msg::DiscoverMessage::SetTargetsHeight { height },
                ));
                let result = process_message_round(effect_runner, action_rx, msg, state);
                *dirty |= result.dirty;
            }
        }
    }
    if *splitter_drag == Some(SplitterDrag::Explorer) {
        let size = terminal.size()?;
        let body_top = 3u16;
        let body_h = size
            .height
            .saturating_sub(body_top)
            .saturating_sub(footer_view::footer_height(&state.footer, size.width));
        let inner = app_explorer_rect(size, body_top, body_h, state).map(|explorer| {
            Rect::new(
                explorer.x.saturating_add(1),
                explorer.y.saturating_add(1),
                explorer.width.saturating_sub(2),
                explorer.height.saturating_sub(2),
            )
        });
        if body_h >= 3
            && let Some(inner) = inner
            && inner.height >= 3
        {
            let height =
                crate::features::explorer::splitter::view::instances_height_for_y(inner, point.y);
            let msg = AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
                crate::features::explorer::msg::ExplorerMessage::SetInstancesHeight { height },
            ));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }
    if *splitter_drag == Some(SplitterDrag::App) {
        let size = terminal.size()?;
        let body_top = 3u16;
        let body_h = size
            .height
            .saturating_sub(body_top)
            .saturating_sub(footer_view::footer_height(&state.footer, size.width));
        if body_h >= 3 {
            let body_area = Rect::new(0, body_top, size.width, body_h);
            let width =
                crate::features::app_splitter::view::explorer_width_for_x(body_area, point.x);
            let msg = AppMsg::SetExplorerWidth(width);
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }
    if let Some(SplitterDrag::Sql(tab_id, splitter)) = *splitter_drag {
        tracing::trace!(?splitter, ?point, "drag move begin");
        let size = terminal.size()?;
        // The feature resolves the drag to a resize
        // message; the shell only supplies the area and
        // the coordinates.
        if let Some(tab_area) = sql_tab_area_for_hit(size, state)
            && let Some(msg) =
                crate::features::sql_workspace::sql_tab::splitter::view::sql_tab_splitter_resize_msg(
                    &state.sql.sql_tab,
                    tab_area,
                    tab_id,
                    splitter,
                    point.x,
                    point.y,
                )
        {
            let msg = AppMsg::Sql(crate::features::sql_workspace::msg::SqlMsg::Message(
                crate::features::sql_workspace::msg::SqlMessage::SqlTab(
                    crate::features::sql_workspace::sql_tab::msg::SqlTabMsg::Message(msg),
                ),
            ));
            tracing::debug!(?msg, "dispatch resize msg");
            let result = process_message_round(effect_runner, action_rx, msg, state);
            tracing::debug!("resize msg processed");
            *dirty |= result.dirty;
        }
    }

    // History list h_scrollbar drag: convert the current
    // absolute mouse x to a position inside the track
    // using the track geometry captured at Down time.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::HistoryH
    {
        let position = drag.offset_for_pointer(point.x, point.y);
        if let Some(active_tab) = state.sql.sql_tab.active_tab {
            use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
            use crate::features::sql_workspace::sql_tab::history::msg::{
                HistoryMessage, HistoryMsg,
            };
            use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
            let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::History {
                    tab_id: active_tab,
                    msg: HistoryMsg::Message(HistoryMessage::SetHScroll { position }),
                },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        } else {
            state.scrollbar_drag = None;
        }
    }

    // History list v_scrollbar drag: scrollbar position
    // IS the viewport start. Send SetVScroll directly.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::HistoryV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        if let Some(active_tab) = state.sql.sql_tab.active_tab {
            use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
            use crate::features::sql_workspace::sql_tab::history::msg::{
                HistoryMessage, HistoryMsg,
            };
            use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
            let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::History {
                    tab_id: active_tab,
                    msg: HistoryMsg::Message(HistoryMessage::SetVScroll { position: start }),
                },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        } else {
            state.scrollbar_drag = None;
        }
    }

    // Editor body v_scrollbar drag: same linear mapping as
    // history — scrollbar thumb position IS the viewport start.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::SqlV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        if let Some(active_tab) = state.sql.sql_tab.active_tab {
            use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
            use crate::features::sql_workspace::sql_tab::editor::msg::{EditorMessage, EditorMsg};
            use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
            let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    tab_id: active_tab,
                    msg: EditorMsg::Message(EditorMessage::SetVScroll { position: start }),
                },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        } else {
            state.scrollbar_drag = None;
        }
    }

    // Results list h_scrollbar drag: convert mouse x
    // inside the track to a horizontal scroll position.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::ResultsH
    {
        let position = drag.offset_for_pointer(point.x, point.y);
        if let Some(active_tab) = state.sql.sql_tab.active_tab {
            use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
            use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
            use crate::features::sql_workspace::sql_tab::results::msg::{
                ResultsMessage, ResultsMsg,
            };
            let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Results {
                    tab_id: active_tab,
                    msg: ResultsMsg::Message(ResultsMessage::SetHScroll { position }),
                },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        } else {
            state.scrollbar_drag = None;
        }
    }

    // Results list v_scrollbar drag: scrollbar position
    // IS the viewport start. Send SetVScroll directly.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::ResultsV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        if let Some(active_tab) = state.sql.sql_tab.active_tab {
            use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
            use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
            use crate::features::sql_workspace::sql_tab::results::msg::{
                ResultsMessage, ResultsMsg,
            };
            let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Results {
                    tab_id: active_tab,
                    msg: ResultsMsg::Message(ResultsMessage::SetVScroll { position: start }),
                },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        } else {
            state.scrollbar_drag = None;
        }
    }

    // Results column-width resize drag: convert the mouse x to the target
    // width of the dragged column using the shared list
    // geometry.
    if let Some(col) = state.splitter_hover.results_col_resize_drag {
        use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
        use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
        use crate::features::sql_workspace::sql_tab::results::msg::{ResultsMessage, ResultsMsg};
        let size = terminal.size()?;
        if let Some(tab_area) = sql_tab_area_for_hit(size, state)
            && let Some(active_tab) = state.sql.sql_tab.active_tab()
            && let Some(list_inner) =
                crate::features::sql_workspace::sql_tab::view::results_list_rect(
                    active_tab, tab_area,
                )
        {
            let width =
                crate::features::sql_workspace::sql_tab::results::list::view::col_width_from_drag_x(
                    list_inner,
                    &active_tab.results.list,
                    col,
                    point.x,
                );
            let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Results {
                    tab_id: active_tab.session.id,
                    msg: ResultsMsg::Message(ResultsMessage::AdjustColWidthTo { col, width }),
                },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
        }
    }

    // Discover targets v_scrollbar drag: same linear
    // mapping as history/results.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::DiscoverTargetsV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        use crate::features::discover::msg::{DiscoverMessage, DiscoverMsg};
        use crate::features::discover::targets::msg::{TargetsMessage, TargetsMsg};
        let msg = AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Targets(
            TargetsMsg::Message(TargetsMessage::SetVScroll { position: start }),
        )));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }

    // Discover results v_scrollbar drag: same pattern.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::DiscoverResultsV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        use crate::features::discover::msg::{DiscoverMessage, DiscoverMsg};
        use crate::features::discover::results::msg::{ResultsMessage, ResultsMsg};
        let msg = AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Results(
            ResultsMsg::Message(ResultsMessage::SetVScroll { position: start }),
        )));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }

    // Explorer objects v_scrollbar drag.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::ObjectsV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        use crate::features::explorer::objects::msg::{ObjectsMessage, ObjectsMsg};
        let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Objects(
            ObjectsMsg::Message(ObjectsMessage::SetVScroll { position: start }),
        )));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }

    // Explorer instances v_scrollbar drag.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::TreeV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
            InstancesMsg::Message(InstancesMessage::SetVScroll { position: start }),
        )));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }

    // Explorer objects h_scrollbar drag.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::ObjectsH
    {
        let position = drag.offset_for_pointer(point.x, point.y);
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        use crate::features::explorer::objects::msg::{ObjectsMessage, ObjectsMsg};
        let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Objects(
            ObjectsMsg::Message(ObjectsMessage::SetHScroll { position }),
        )));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }

    // Explorer instances h_scrollbar drag.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::TreeH
    {
        let position = drag.offset_for_pointer(point.x, point.y);
        use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
        let msg = AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
            InstancesMsg::Message(InstancesMessage::SetHScroll { position }),
        )));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }

    // IW connections v_scrollbar drag.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::ConnectionsV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        use crate::features::instance_workspace::connections::msg::{
            ConnectionsMessage, ConnectionsMsg,
        };
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
            ConnectionsMsg::Message(ConnectionsMessage::SetVScroll { position: start }),
        )));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }

    // IW overview v_scrollbar drag.
    if let Some(drag) = state.scrollbar_drag
        && drag.which == ActiveScrollbar::OverviewV
    {
        let start = drag.offset_for_pointer(point.x, point.y);
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        use crate::features::instance_workspace::overview::msg::{OverviewMessage, OverviewMsg};
        let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Overview(OverviewMsg::Message(
            OverviewMessage::SetVScroll { position: start },
        ))));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }
    Ok(())
}

/// Button release: end every held-button drag, then re-evaluate which
/// splitter (if any) the cursor is now over.
fn handle_up(
    mouse: &MouseEvent,
    terminal: &mut AppTerminal,
    state: &mut AppState,
    dirty: &mut bool,
    splitter_drag: &mut Option<SplitterDrag>,
) -> anyhow::Result<()> {
    // One drag slot covers every scrollbar, so releasing
    // the button ends whichever one was active — no need
    // to clear a flag per bar.
    // Releasing the button ends every held-button drag at
    // once (scrollbar, splitters, results column-resize).
    // The debug logs below are kept for diagnostics; the
    // state clear itself is delegated to `clear_active_drags`.
    // `take()` *ends* the drag: the target is cleared,
    // not merely logged, so a finished drag cannot leak
    // into the next one. The old per-splitter booleans
    // were only read here and never reset, so every
    // later drag also resized whatever had been dragged
    // before (a vertical move resizing a horizontal
    // split and vice versa).
    match splitter_drag.take() {
        Some(SplitterDrag::Explorer) => {
            tracing::debug!("explorer instances/objects splitter drag finished")
        }
        Some(SplitterDrag::Discover) => {
            tracing::debug!("discover targets/results splitter drag finished")
        }
        Some(SplitterDrag::App) => {
            tracing::debug!("app explorer/workspace splitter drag finished")
        }
        Some(SplitterDrag::Sql(_, splitter)) => {
            tracing::debug!(?splitter, "splitter drag finished")
        }
        None => {}
    }
    let col_resize = state.splitter_hover.results_col_resize_drag;
    if col_resize.is_some() {
        tracing::debug!(col = col_resize, "results column resize drag finished");
    }
    // Ending a drag *changes the rendered highlight* (the
    // scrollbar thumb / splitter drops its accent color),
    // so force a repaint even when the release point is not
    // over a splitter. Without this the cleared
    // `scrollbar_drag` / `dragging_flags` would not be
    // painted and the "active" highlight would stay stuck
    // on screen (the `Up` only set `dirty` via hover, which
    // is false over a bare scrollbar).
    let was_dragging = state.scrollbar_drag.is_some()
        || state.splitter_hover.results_col_resize_drag.is_some()
        || state.splitter_hover.dragging_flags().iter().any(|&f| f);
    clear_active_drags(state);
    *dirty |= was_dragging;
    // Re-evaluate hover after drag end — the cursor may
    // still be over a splitter.
    let size = terminal.size()?;
    if update_splitter_hover(state, mouse.column, mouse.row, size) {
        *dirty = true;
    }
    Ok(())
}

/// Hover with no button held: end a drag whose `Up` was missed (released
/// outside the window), then re-evaluate the splitter hover highlight.
fn handle_moved(
    mouse: &MouseEvent,
    terminal: &mut AppTerminal,
    state: &mut AppState,
    dirty: &mut bool,
    splitter_drag: &mut Option<SplitterDrag>,
) -> anyhow::Result<()> {
    // A `Moved` event means *no* mouse button is pressed
    // (crossterm reports button-held motion as `Drag`). If
    // a drag is still recorded here, its `Up` was missed —
    // e.g. the button was released *outside* the terminal
    // window, so the terminal never delivered the release
    // and the highlight would stay stuck. End the drag now
    // and force a repaint so the accent color resets. Any
    // real (button-held) drag produces `Drag` events, never
    // `Moved`, so this can never fire mid-drag.
    if state.scrollbar_drag.is_some()
        || splitter_drag.is_some()
        || state.splitter_hover.results_col_resize_drag.is_some()
        || state.splitter_hover.dragging_flags().iter().any(|&f| f)
    {
        *splitter_drag = None;
        clear_active_drags(state);
        *dirty = true;
    }
    // Hover is a continuous gesture: only request a
    // redraw when some hover bit actually toggles,
    // avoiding wasteful repaints on every mouse pixel.
    let size = terminal.size()?;
    if update_splitter_hover(state, mouse.column, mouse.row, size) {
        *dirty = true;
    }
    Ok(())
}

/// The app body layout (explorer + vertical splitter + workspace), computed once
/// from the same `app_body_layout` the render uses. This is the single geometry
/// source for app-level mouse hit-testing, so every region it derives (explorer,
/// workspace) agrees with the rendered splitter (no hard-coded 20% drift when
/// the Explorer is resized).
pub(crate) fn app_body_geometry(
    size: ratatui::layout::Size,
    body_top: u16,
    body_h: u16,
    state: &AppState,
) -> Option<crate::features::app_splitter::view::AppBodyLayout> {
    let body_area = Rect::new(0, body_top, size.width, body_h);
    let layout = crate::features::app_splitter::view::app_body_layout(
        body_area,
        state.splitter.explorer_pane_width,
    );
    (layout.workspace.width > 0 && layout.explorer.width > 0).then_some(layout)
}

/// The workspace region of the app body (right of the Explorer / workspace
/// splitter). Returns `None` when the body is too small to lay out both panes.
pub(crate) fn workspace_rect_for_hit(
    size: ratatui::layout::Size,
    body_top: u16,
    body_h: u16,
    state: &AppState,
) -> Option<ratatui::layout::Rect> {
    app_body_geometry(size, body_top, body_h, state).map(|g| g.workspace)
}

/// The Explorer column rect (the left pane of the app body splitter).
pub(crate) fn app_explorer_rect(
    size: ratatui::layout::Size,
    body_top: u16,
    body_h: u16,
    state: &AppState,
) -> Option<ratatui::layout::Rect> {
    app_body_geometry(size, body_top, body_h, state).map(|g| g.explorer)
}

/// Compute the SQL tab region (tab bar + child panes) for mouse hit-testing,
/// mirroring `sql_workspace/view.rs` (workspace inner minus its tab footer).
/// Returns `None` when the SQL workspace is not the region being shown.
pub(crate) fn sql_tab_area_for_hit(
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
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    if body_h < 3 {
        return None;
    }
    let workspace = workspace_rect_for_hit(size, body_top, body_h, state)?;
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

/// Resolve which splitter a press at `(x, y)` starts a drag on.
///
/// Covers the splitters hit-tested **after** the click has re-mapped focus —
/// the app-level Explorer/workspace one, the Explorer instances/objects one and
/// the SQL-tab ones. (The discover splitter is resolved earlier, while focus is
/// still the pre-click one, so it is deliberately not part of this function.)
///
/// Candidates are checked in a fixed priority order and the first hit wins, so
/// a press arms **at most one** splitter: a single drag can never resize two
/// splits, and each target later reads only the axis it owns. Every candidate
/// is gated on actually being rendered, so a drag cannot start on a splitter
/// the user cannot see.
pub(crate) fn resolve_splitter_drag(
    state: &AppState,
    size: ratatui::layout::Size,
    x: u16,
    y: u16,
) -> Option<SplitterDrag> {
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_view::footer_height(&state.footer, size.width));
    if body_h < 3 {
        return None;
    }

    // The app-level Explorer / workspace splitter is draggable from any focus
    // pane: it separates two peer top-level panes rather than the panes of one
    // feature.
    {
        let body_area = Rect::new(0, body_top, size.width, body_h);
        let layout = crate::features::app_splitter::view::app_body_layout(
            body_area,
            state.splitter.explorer_pane_width,
        );
        if crate::features::app_splitter::view::splitter_at(&layout, x, y) {
            return Some(SplitterDrag::App);
        }
    }

    // The Explorer instances / objects splitter: only while the Explorer owns
    // focus, since the split lives inside the Explorer pane.
    if matches!(state.focus, Pane::Explorer(_))
        && let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
        && explorer.height >= 3
    {
        let inner = Rect::new(
            explorer.x.saturating_add(1),
            explorer.y.saturating_add(1),
            explorer.width.saturating_sub(2),
            explorer.height.saturating_sub(2),
        );
        let layout = crate::features::explorer::splitter::view::explorer_body_layout(
            inner,
            state.explorer.splitter.instances_height,
        );
        if crate::features::explorer::splitter::view::splitter_at(&layout, x, y) {
            return Some(SplitterDrag::Explorer);
        }
    }

    // The SQL-tab splitters: only while the SQL workspace owns focus. The
    // feature resolves the point to a splitter; the shell only supplies the
    // area and the coordinates.
    if state.focus == Pane::SQLWorkspace
        && let Some(tab_area) = sql_tab_area_for_hit(size, state)
        && let Some((tab_id, splitter)) =
            crate::features::sql_workspace::sql_tab::splitter::view::sql_tab_splitter_at(
                &state.sql.sql_tab,
                tab_area,
                x,
                y,
            )
    {
        return Some(SplitterDrag::Sql(tab_id, splitter));
    }

    None
}

/// Compute the Instance Workspace's **body** rect (active sub-pane content,
/// inside the tab bar and parent footer), or `None` when IW is not shown.
/// Mirrors `instance_workspace/view.rs`'s area splitting logic.
pub(crate) fn iw_body_area_for_hit(
    size: ratatui::layout::Size,
    state: &AppState,
) -> Option<ratatui::layout::Rect> {
    if !state.explorer.instances.active_is_instance()
        || state.modal.is_some()
        || matches!(state.focus, Pane::Discover(_))
    {
        return None;
    }
    let footer_h = footer_view::footer_height(&state.footer, size.width);
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    if body_h < 5 {
        return None;
    }
    let workspace = workspace_rect_for_hit(size, body_top, body_h, state)?;
    // IW outer Block (1 col/row border).
    let inner = ratatui::layout::Rect::new(
        workspace.x.saturating_add(1),
        workspace.y.saturating_add(1),
        workspace.width.saturating_sub(2),
        workspace.height.saturating_sub(2),
    );
    if inner.width == 0 || inner.height < 3 {
        return None;
    }
    // Footer height matches instance_workspace/view.rs's wrapped-line-count
    // calculation for IW's pane footer + overview status (overview status only
    // when the active tab is Overview).
    let mut footer_text = crate::common::view::hints::instance_workspace_footer_text(state.iw.pane);
    if matches!(state.iw.pane, crate::app_shell::nav::IwPane::Overview)
        && let Some(status) = state.iw.overview.status.as_deref()
        && !status.is_empty()
    {
        footer_text.push('\n');
        footer_text.push_str(status);
    }
    use crate::common::utils::text_width::wrapped_line_count;
    let footer_h = wrapped_line_count(&footer_text, inner.width)
        .max(1)
        .min(inner.height.saturating_sub(2).max(1));
    // Split: tab bar (1) + body + footer.
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Min(0),
            ratatui::layout::Constraint::Length(footer_h),
        ])
        .split(inner);
    Some(chunks[1])
}

/// Compute the context picker overlay rect (in the active tab's editor) for
/// mouse hit-testing, or `None` when the picker is closed / not shown.
pub(crate) fn sql_picker_area_for_hit(
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
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    if body_h < 3 {
        return None;
    }
    let workspace = workspace_rect_for_hit(size, body_top, body_h, state)?;
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
        tab.splitter.editor_top_height,
        tab.splitter.history_pane_width,
    );
    crate::features::sql_workspace::sql_tab::editor::view::context_picker_area(layout.editor, true)
}
/// Columns scrolled per horizontal wheel tick.
///
/// Horizontal content is typically far wider than it is tall, so reusing the
/// vertical wheel's one-cell-per-tick granularity would make a wide table
/// impractical to traverse. Three columns per tick keeps a gesture useful
/// while staying fine-grained enough to land on a column.
const WHEEL_H_STEP: i32 = 3;

/// Resolve a wheel tick into `(horizontal, delta)`.
///
/// `delta` is `-1`/`+1` per physical tick; callers scale it to the step size
/// their pane wants.
///
/// Shift+wheel is the platform-wide convention for horizontal scrolling, so a
/// *vertical* tick carrying SHIFT is reinterpreted on the horizontal axis. Some
/// terminals instead report the gesture natively as `ScrollLeft`/`ScrollRight`
/// with no modifier at all, which is horizontal either way — hence the two
/// spellings of "scroll sideways" both map to `horizontal == true`.
pub(crate) fn wheel_axis(
    kind: crossterm::event::MouseEventKind,
    modifiers: crossterm::event::KeyModifiers,
) -> (bool, i32) {
    let (vertical, delta) = match kind {
        crossterm::event::MouseEventKind::ScrollUp => (true, -1),
        crossterm::event::MouseEventKind::ScrollDown => (true, 1),
        crossterm::event::MouseEventKind::ScrollLeft => (false, -1),
        crossterm::event::MouseEventKind::ScrollRight => (false, 1),
        // Not a wheel tick — every caller's match guard admits only the four
        // kinds above, so this is unreachable in practice.
        _ => (true, 0),
    };
    let horizontal = !vertical || modifiers.contains(crossterm::event::KeyModifiers::SHIFT);
    (horizontal, delta)
}

/// Compute the two explorer child tree areas (instances top / objects bottom)
/// from the explorer's outer rect, mirroring `explorer/view.rs` (the stored
/// instances height + 1-row splitter, inside the outer border).
pub(crate) fn explorer_child_areas(
    explorer: ratatui::layout::Rect,
    instances_height: u16,
) -> (ratatui::layout::Rect, ratatui::layout::Rect) {
    // Mirror the explorer render exactly: the two child panes are laid out with
    // the shared `explorer_body_layout` inside the outer border. Computing them
    // the same way here guarantees the click hit-testing uses the same
    // child-pane rectangles the render draws, so a click on a row maps to the
    // same row (no off-by-one drift).
    let inner = Rect::new(
        explorer.x.saturating_add(1),
        explorer.y.saturating_add(1),
        explorer.width.saturating_sub(2),
        explorer.height.saturating_sub(2),
    );
    let panes =
        crate::features::explorer::splitter::view::explorer_body_layout(inner, instances_height);
    (panes.instances, panes.objects)
}

/// Build the explorer messages for a single click on a visible tree row.
/// Whether the click at `(x, y)` landed on an expand/collapse marker in the
/// explorer's instances/objects tree. Used to suppress the double-click "open"
/// (Select) action on a marker click: clicking the arrow, single or double,
/// must only expand/collapse.
pub(crate) fn is_explorer_toggle_click(
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
    let (instances_area, objects_area) =
        explorer_child_areas(explorer, state.explorer.splitter.instances_height);
    match explorer_pane_for_click(
        y,
        body_top,
        body_h,
        state.explorer.splitter.instances_height,
    ) {
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
pub(crate) fn explorer_click_hits_row(
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
    let (instances_area, objects_area) =
        explorer_child_areas(explorer, state.explorer.splitter.instances_height);
    match explorer_pane_for_click(
        y,
        body_top,
        body_h,
        state.explorer.splitter.instances_height,
    ) {
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
pub(crate) fn explorer_row_click_msgs(
    size: ratatui::layout::Size,
    body_top: u16,
    body_h: u16,
    x: u16,
    y: u16,
    state: &AppState,
) -> Option<Vec<AppMsg>> {
    use crate::features::explorer::instances::msg::InstancesMessage;
    use crate::features::explorer::objects::msg::ObjectsMessage;
    // Use the live Explorer column width so a click inside the (resizable)
    // Explorer is mapped with the same geometry the render draws.
    let explorer = app_explorer_rect(size, body_top, body_h, state)?;
    if x < explorer.x || x >= explorer.right() {
        return None;
    }
    let (instances_area, objects_area) =
        explorer_child_areas(explorer, state.explorer.splitter.instances_height);
    let pane = explorer_pane_for_click(
        y,
        body_top,
        body_h,
        state.explorer.splitter.instances_height,
    );
    match pane {
        crate::app_shell::nav::ExplorerPane::Instances => {
            let inst = &state.explorer.instances;
            let row = crate::features::explorer::instances::view::row_at(instances_area, inst, y)?;
            let jump = instances_msg(InstancesMessage::JumpTo { row });
            // Clicking the expand/collapse marker on an instance row toggles its
            // expansion (not Select, which would open the workspace). Need the
            // row's instance index and current state.
            if crate::features::explorer::instances::view::toggle_at(instances_area, inst, x, y)
                .is_some()
            {
                // Clicking the expand/collapse marker toggles that instance's
                // expansion without moving the cursor (no `jump`).
                return Some(vec![instances_msg(InstancesMessage::ToggleExpandAt {
                    row,
                })]);
            }
            Some(vec![jump])
        }
        crate::app_shell::nav::ExplorerPane::Objects => {
            let objs = &state.explorer.objects;
            let row = crate::features::explorer::objects::view::row_at(objects_area, objs, y)?;
            let jump = objects_msg(ObjectsMessage::JumpTo { row });
            // Clicking the expand/collapse marker toggles that database/group's
            // expansion without moving the cursor (no `jump`).
            if crate::features::explorer::objects::view::toggle_at(objects_area, objs, x, y)
                .is_some()
            {
                return Some(vec![objects_msg(ObjectsMessage::ToggleExpandAt { row })]);
            }
            Some(vec![jump])
        }
    }
}

pub(crate) fn instances_msg(
    m: crate::features::explorer::instances::msg::InstancesMessage,
) -> AppMsg {
    AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
        crate::features::explorer::msg::ExplorerMessage::Instances(
            crate::features::explorer::instances::msg::InstancesMsg::Message(m),
        ),
    ))
}

pub(crate) fn objects_msg(m: crate::features::explorer::objects::msg::ObjectsMessage) -> AppMsg {
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
pub(crate) fn explorer_pane_for_click(
    row: u16,
    body_top: u16,
    body_h: u16,
    instances_height: u16,
) -> crate::app_shell::nav::ExplorerPane {
    use crate::app_shell::nav::ExplorerPane;
    // Use the same `Layout` as the render and `explorer_child_areas` so the
    // instances/objects boundary matches exactly (no `height/2` vs `Layout`
    // rounding drift).
    let explorer = Rect::new(0, body_top, 1, body_h);
    let (instances, _objects) = explorer_child_areas(explorer, instances_height);
    if row <= instances.y.saturating_add(instances.height) {
        ExplorerPane::Instances
    } else {
        ExplorerPane::Objects
    }
}

/// Refresh each horizontal splitter's last-laid-out track so keyboard `+`/`-`
/// nudges clamp against the live body height. The tracks are approximated from
/// the terminal size (header/footer/border/tab-bar rows); the exact layout
/// re-clamps at render time anyway, and a small approximation error only shifts
/// where a nudge stops — never the rendered split.
pub(crate) fn normalize_splitter_tracks(
    state: &mut crate::app::state::AppState,
    size: ratatui::layout::Size,
) {
    let footer_h = footer_view::footer_height(&state.footer, size.width);
    // Body height: header (3) at the top, footer at the bottom.
    let body_h = size.height.saturating_sub(3).saturating_sub(footer_h);
    // SQL tab body track: workspace inner, minus the workspace footer and the
    // tab bar. Computed with the same footer logic the workspace render uses.
    let workspace = workspace_rect_for_hit(size, 3, body_h, state);
    // Editor+history track width (the workspace inner width), matching the
    // render (live Explorer splitter width), so `[`/`]` history nudges clamp
    // against the exact editor min width.
    let sql_track_w = workspace.map(|ws| ws.width.saturating_sub(2)).unwrap_or(
        size.width
            .saturating_sub(size.width.saturating_mul(2) / 10)
            .saturating_sub(2),
    );
    let sql_body_h = workspace
        .map(|ws| {
            let inner_w = ws.width.saturating_sub(2);
            let ws_footer_h = crate::common::view::hints::footer_height(
                &crate::common::view::hints::sql_workspace_footer_text(),
                inner_w.max(1),
            );
            ws.height
                .saturating_sub(2)
                .saturating_sub(ws_footer_h)
                .saturating_sub(1)
        })
        .unwrap_or(body_h.saturating_sub(4));
    // Derive the SQL splitter bounds from the layout itself (the same function
    // the render uses), so nudge/drag clamp to the exact rendered boundary.
    for tab in &mut state.sql.sql_tab.tabs {
        let sql_area = Rect::new(0, 0, sql_track_w.max(1), sql_body_h.max(1));
        let layout = crate::features::sql_workspace::sql_tab::layout::sql_tab_layout(
            sql_area,
            tab.splitter.editor_top_height,
            tab.splitter.history_pane_width,
        );
        tab.splitter.editor_top_min = layout.editor_top_min;
        tab.splitter.editor_top_max = layout.editor_top_max.max(layout.editor_top_min);
        tab.splitter.history_min = layout.history_min;
        tab.splitter.history_max = layout.history_max.max(layout.history_min);
    }
    // Explorer instances/objects bounds, derived from the same layout the render
    // uses so nudge clamps to the exact rendered boundary.
    if let Some(explorer) = app_explorer_rect(size, 3, body_h, state) {
        let inner = Rect::new(
            explorer.x + 1,
            explorer.y + 1,
            explorer.width.saturating_sub(2),
            explorer.height.saturating_sub(2),
        );
        let layout = crate::features::explorer::splitter::view::explorer_body_layout(
            inner,
            state.explorer.splitter.instances_height,
        );
        state.explorer.splitter.instances_min = layout.instances_min;
        state.explorer.splitter.instances_max = layout.instances_max.max(layout.instances_min);
    }
    // Discover targets/results bounds, from the same popup/body the render lays.
    if let Some(ws) = workspace {
        let discover_popup = crate::common::view::modal::popup_rect(ws, 75, 75);
        let body =
            crate::features::discover::view::discover_body_area(discover_popup, &state.discover);
        let layout = crate::features::discover::splitter::view::discover_body_layout(
            body,
            state.discover.splitter.targets_height,
        );
        state.discover.splitter.targets_min = layout.targets_min;
        state.discover.splitter.targets_max = layout.targets_max.max(layout.targets_min);
    }
}

/// Update splitter hover highlight state from a mouse position.
///
/// Hit-tests every visible splitter against `(x, y)` using the same geometry
/// the renderers employ, so hover highlights exactly the same line that is
/// drawn. Returns `true` when any hover bit actually changed (used to decide
/// whether to request a redraw).
pub(crate) fn update_splitter_hover(
    state: &mut crate::app::state::AppState,
    x: u16,
    y: u16,
    size: ratatui::layout::Size,
) -> bool {
    use crate::common::view::splitter::hit;

    let before = state.splitter_hover;

    // No hover highlight while a modal or the discover close-confirm is active.
    let can_hover = state.modal.is_none() && !state.discover.close_confirm;

    // Preserve the active drag flags — they are managed by the Down/Drag/Up
    // event handlers and must survive a hover recompute (e.g. a `Moved` event
    // arriving mid-drag). Only the hover bits are recomputed here.
    let drag = before.dragging_flags();
    let results_col_resize_drag = before.results_col_resize_drag;
    state.splitter_hover = crate::app::state::SplitterHoverState::default();
    state.splitter_hover.set_dragging_flags(drag);
    state.splitter_hover.results_col_resize_drag = results_col_resize_drag;

    if !can_hover {
        return before != state.splitter_hover;
    }

    let footer_h = footer_view::footer_height(&state.footer, size.width);
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    if body_h < 3 {
        return before != state.splitter_hover;
    }

    // --- App-level Explorer / workspace vertical splitter ---
    if let Some(layout) = app_body_geometry(size, body_top, body_h, state) {
        state.splitter_hover.app_splitter = hit(layout.v_splitter, x, y);
    }

    // --- Explorer instances / objects horizontal splitter ---
    if matches!(state.focus, Pane::Explorer(_))
        && let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
    {
        let inner = Rect::new(
            explorer.x.saturating_add(1),
            explorer.y.saturating_add(1),
            explorer.width.saturating_sub(2),
            explorer.height.saturating_sub(2),
        );
        if inner.height >= 3 {
            let layout = crate::features::explorer::splitter::view::explorer_body_layout(
                inner,
                state.explorer.splitter.instances_height,
            );
            state.splitter_hover.explorer_splitter = hit(layout.splitter, x, y);
        }
    }

    // --- Discover targets / results horizontal splitter ---
    if matches!(state.focus, Pane::Discover(_))
        && let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state)
    {
        let discover_popup = crate::common::view::modal::popup_rect(workspace, 75, 75);
        let body =
            crate::features::discover::view::discover_body_area(discover_popup, &state.discover);
        if body.height >= 3 {
            let layout = crate::features::discover::splitter::view::discover_body_layout(
                body,
                state.discover.splitter.targets_height,
            );
            state.splitter_hover.discover_splitter = hit(layout.splitter, x, y);
        }
    }

    // --- SQL tab splitters ---
    if state.focus == Pane::SQLWorkspace
        && let Some(tab_area) = sql_tab_area_for_hit(size, state)
        && let Some((_tab_id, splitter)) =
            crate::features::sql_workspace::sql_tab::splitter::view::sql_tab_splitter_at(
                &state.sql.sql_tab,
                tab_area,
                x,
                y,
            )
    {
        // Hit-test resolved — mark the corresponding hover bit.
        match splitter {
            crate::features::sql_workspace::sql_tab::splitter::view::SqlSplitter::EditorResults => {
                state.splitter_hover.sql_editor_results = true;
            }
            crate::features::sql_workspace::sql_tab::splitter::view::SqlSplitter::EditorHistory => {
                state.splitter_hover.sql_editor_history = true;
            }
            crate::features::sql_workspace::sql_tab::splitter::view::SqlSplitter::HistoryDetail => {
                state.splitter_hover.history_detail = true;
            }
            crate::features::sql_workspace::sql_tab::splitter::view::SqlSplitter::ResultsDetail => {
                state.splitter_hover.results_detail = true;
            }
        }
    }

    // Results column-width resize hover: a pointer over a result header
    // splitter highlights that column's border so the resizable region is
    // visible. Uses the same shared list geometry as the render.
    if state.focus == Pane::SQLWorkspace
        && let Some(tab_area) = sql_tab_area_for_hit(size, state)
        && let Some(active_tab) = state.sql.sql_tab.active_tab()
        && let Some(list_inner) =
            crate::features::sql_workspace::sql_tab::view::results_list_rect(active_tab, tab_area)
    {
        state.splitter_hover.results_col_resize_hover =
            crate::features::sql_workspace::sql_tab::results::list::view::col_resize_hit_at(
                list_inner,
                &active_tab.results.list,
                x,
                y,
            );
    }

    before != state.splitter_hover
}

/// Map a click inside the discover popup to a discover child sub-pane
/// (engine / targets / results), mirroring the discover view's vertical layout
/// and Ctrl+j/k. Uses the same live workspace/popup/engine geometry as the
/// render, so a click maps to the same pane that is drawn. Returns `None` for
/// clicks outside the popup.
pub(crate) fn discover_subpane_for_click(
    col: u16,
    row: u16,
    workspace: Rect,
    state: &crate::features::discover::state::DiscoverState,
) -> Option<crate::app_shell::nav::DiscoverPane> {
    use crate::app_shell::nav::DiscoverPane;
    let popup = crate::common::view::modal::popup_rect(workspace, 75, 75);
    if col < popup.x || col >= popup.right() {
        return None;
    }
    let body = crate::features::discover::view::discover_body_area(popup, state);
    // The engine selector occupies the rows between the popup top and the body.
    if row >= popup.y && row < body.y {
        return Some(DiscoverPane::Engine);
    }
    if row < body.y || row >= body.bottom() {
        return None;
    }
    // Split the body at the same boundary the splitter renders at (the current
    // targets height, clamped to the live track), so clicking agrees with the
    // rendered splitter.
    let layout = crate::features::discover::splitter::view::discover_body_layout(
        body,
        state.splitter.targets_height,
    );
    if row < layout.targets.bottom() {
        Some(DiscoverPane::Targets)
    } else {
        Some(DiscoverPane::Results)
    }
}

/// Build the workspace messages for a SQL click action. A double-click on a
/// picker row yields both a cursor jump and an apply, so a `Vec` is returned.
pub(crate) fn sql_click_msgs(
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
        editor_msg(
            tab_id,
            EditorMessage::ContextPicker(ContextPickerMsg::Message(m)),
        )
    };
    let close = |tab_id: usize| picker(tab_id, ContextPickerMessage::Close);

    match action {
        SqlClickAction::FocusSubPane(focus) => {
            vec![AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                SqlTabMsg::Message(SqlTabMessage::Focus(focus)),
            )))]
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
        SqlClickAction::ContextPickerHit {
            column,
            cursor,
            double,
        } => {
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
        SqlClickAction::HistoryApply => {
            let Some(tab_id) = tab_id(sql.active_tab) else {
                return Vec::new();
            };
            use crate::features::sql_workspace::sql_tab::history::msg::{
                HistoryMessage, HistoryMsg,
            };
            vec![AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                SqlTabMsg::Message(SqlTabMessage::History {
                    tab_id,
                    msg: HistoryMsg::Message(HistoryMessage::Apply),
                }),
            )))]
        }
        SqlClickAction::HistoryRowClicked { index } => {
            let Some(tab_id) = tab_id(sql.active_tab) else {
                return Vec::new();
            };
            use crate::features::sql_workspace::sql_tab::history::msg::{
                HistoryMessage, HistoryMsg,
            };
            use crate::features::sql_workspace::sql_tab::state::SqlFocus;
            let mut msgs = vec![
                // Switch focus to History pane first (no-op if already focused).
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Focus(SqlFocus::History),
                )))),
            ];
            msgs.push(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                SqlTabMsg::Message(SqlTabMessage::History {
                    tab_id,
                    msg: HistoryMsg::Message(HistoryMessage::SetCursor { index }),
                }),
            ))));
            msgs
        }
        SqlClickAction::HistoryHScrollbar {
            track_x,
            x,
            max_scroll,
            viewport_width,
        } => {
            let Some(tab_id) = tab_id(sql.active_tab) else {
                return Vec::new();
            };
            use crate::features::sql_workspace::sql_tab::history::msg::{
                HistoryMessage, HistoryMsg,
            };
            use crate::features::sql_workspace::sql_tab::state::SqlFocus;
            let _rel_x = x.saturating_sub(track_x);
            let position = crate::common::view::pane_scrollbar::scroll_offset_from_track(
                x,
                track_x,
                viewport_width,
                max_scroll,
            );
            vec![
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Focus(SqlFocus::History),
                )))),
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::History {
                        tab_id,
                        msg: HistoryMsg::Message(HistoryMessage::SetHScroll { position }),
                    },
                )))),
            ]
        }
        SqlClickAction::HistoryVScrollbar {
            track_y,
            y,
            max_scroll,
            viewport_height,
        } => {
            let Some(tab_id) = tab_id(sql.active_tab) else {
                return Vec::new();
            };
            use crate::features::sql_workspace::sql_tab::history::msg::{
                HistoryMessage, HistoryMsg,
            };
            use crate::features::sql_workspace::sql_tab::state::SqlFocus;
            let _rel_y = y.saturating_sub(track_y);
            let start = crate::common::view::pane_scrollbar::scroll_offset_from_track(
                y,
                track_y,
                viewport_height,
                max_scroll,
            );
            vec![
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Focus(SqlFocus::History),
                )))),
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::History {
                        tab_id,
                        msg: HistoryMsg::Message(HistoryMessage::SetVScroll { position: start }),
                    },
                )))),
            ]
        }
        SqlClickAction::ResultsCellClicked { row, col } => {
            let Some(tab_id) = tab_id(sql.active_tab) else {
                return Vec::new();
            };
            use crate::features::sql_workspace::sql_tab::results::msg::{
                ResultsMessage, ResultsMsg,
            };
            use crate::features::sql_workspace::sql_tab::state::SqlFocus;
            vec![
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Focus(SqlFocus::Results),
                )))),
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Results {
                        tab_id,
                        msg: ResultsMsg::Message(ResultsMessage::SetSelection { row, col }),
                    },
                )))),
            ]
        }
        SqlClickAction::ResultsOpenDetail => {
            let Some(tab_id) = tab_id(sql.active_tab) else {
                return Vec::new();
            };
            use crate::features::sql_workspace::sql_tab::results::msg::{
                ResultsMessage, ResultsMsg,
            };
            use crate::features::sql_workspace::sql_tab::state::SqlFocus;
            vec![
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Focus(SqlFocus::Results),
                )))),
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Results {
                        tab_id,
                        msg: ResultsMsg::Message(ResultsMessage::ToggleDetail),
                    },
                )))),
            ]
        }
        SqlClickAction::ResultsHScrollbar {
            track_x,
            x,
            max_scroll,
            viewport_width,
        } => {
            let Some(tab_id) = tab_id(sql.active_tab) else {
                return Vec::new();
            };
            use crate::features::sql_workspace::sql_tab::results::msg::{
                ResultsMessage, ResultsMsg,
            };
            use crate::features::sql_workspace::sql_tab::state::SqlFocus;
            let _rel_x = x.saturating_sub(track_x);
            let position = crate::common::view::pane_scrollbar::scroll_offset_from_track(
                x,
                track_x,
                viewport_width,
                max_scroll,
            );
            vec![
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Focus(SqlFocus::Results),
                )))),
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Results {
                        tab_id,
                        msg: ResultsMsg::Message(ResultsMessage::SetHScroll { position }),
                    },
                )))),
            ]
        }
        SqlClickAction::ResultsVScrollbar {
            track_y,
            y,
            max_scroll,
            viewport_height,
        } => {
            let Some(tab_id) = tab_id(sql.active_tab) else {
                return Vec::new();
            };
            use crate::features::sql_workspace::sql_tab::results::msg::{
                ResultsMessage, ResultsMsg,
            };
            use crate::features::sql_workspace::sql_tab::state::SqlFocus;
            let _rel_y = y.saturating_sub(track_y);
            let start = crate::common::view::pane_scrollbar::scroll_offset_from_track(
                y,
                track_y,
                viewport_height,
                max_scroll,
            );
            vec![
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Focus(SqlFocus::Results),
                )))),
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Results {
                        tab_id,
                        msg: ResultsMsg::Message(ResultsMessage::SetVScroll { position: start }),
                    },
                )))),
            ]
        }
        SqlClickAction::ToggleTableCompletion => {
            let Some(tab_id) = tab_id(sql.active_tab) else {
                return Vec::new();
            };
            vec![AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                SqlTabMsg::Message(SqlTabMessage::ToggleTableCompletion { tab_id }),
            )))]
        }
        // A column-width resize drag is handled entirely by the shell's mouse
        // Down/Drag/Up handlers (geometry is computed there); no feature
        // message is dispatched for the initiating click itself.
        SqlClickAction::ResultsColResize { .. } => Vec::new(),
        SqlClickAction::EditorVScrollbar {
            track_y,
            y,
            max_scroll,
            viewport_height,
        } => {
            let Some(tab_id) = tab_id(sql.active_tab) else {
                return Vec::new();
            };
            use crate::features::sql_workspace::sql_tab::editor::msg::{EditorMessage, EditorMsg};
            let _rel_y = y.saturating_sub(track_y);
            let start = crate::common::view::pane_scrollbar::scroll_offset_from_track(
                y,
                track_y,
                viewport_height,
                max_scroll,
            );
            vec![AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                SqlTabMsg::Message(SqlTabMessage::Editor {
                    tab_id,
                    msg: EditorMsg::Message(EditorMessage::SetVScroll { position: start }),
                }),
            )))]
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// The app body geometry (top row, height) that mouse hit-testing is
    /// computed against, for a given terminal size.
    fn test_body(state: &AppState, size: ratatui::layout::Size) -> (u16, u16) {
        let body_top = 3u16;
        let body_h = size
            .height
            .saturating_sub(body_top)
            .saturating_sub(footer_view::footer_height(&state.footer, size.width));
        (body_top, body_h)
    }

    /// The rect of the Explorer instances/objects horizontal splitter.
    fn test_explorer_splitter_rect(
        state: &AppState,
        size: ratatui::layout::Size,
        body_top: u16,
        body_h: u16,
    ) -> Rect {
        let explorer = app_explorer_rect(size, body_top, body_h, state).expect("explorer rect");
        let inner = Rect::new(
            explorer.x.saturating_add(1),
            explorer.y.saturating_add(1),
            explorer.width.saturating_sub(2),
            explorer.height.saturating_sub(2),
        );
        crate::features::explorer::splitter::view::explorer_body_layout(
            inner,
            state.explorer.splitter.instances_height,
        )
        .splitter
    }

    /// One press arms **at most one** splitter. Regression guard: the drag
    /// targets used to be four independent booleans that were never reset on
    /// `Up`, so a drag of one splitter also resized every split dragged earlier
    /// (a vertical move changing a height, a horizontal move changing a width).
    #[test]
    fn resolve_splitter_drag_arms_exactly_one_target() {
        use crate::features::explorer::state::ExplorerPane;

        let mut state = AppState::default();
        state.term_width = 120;
        state.term_height = 40;
        // The Explorer owns focus, so its instances/objects splitter is
        // eligible — the app-level splitter must still win on its own column.
        state.focus = Pane::Explorer(ExplorerPane::Instances);
        let size = ratatui::layout::Size::new(120, 40);
        let (body_top, body_h) = test_body(&state, size);
        let layout = crate::features::app_splitter::view::app_body_layout(
            Rect::new(0, body_top, size.width, body_h),
            state.splitter.explorer_pane_width,
        );

        // On the app-level vertical splitter: only the app splitter arms.
        assert_eq!(
            resolve_splitter_drag(&state, size, layout.v_splitter.x, layout.v_splitter.y + 5),
            Some(SplitterDrag::App)
        );

        // On the Explorer's horizontal splitter: only that one arms.
        let ex = test_explorer_splitter_rect(&state, size, body_top, body_h);
        assert_eq!(
            resolve_splitter_drag(&state, size, ex.x + 3, ex.y),
            Some(SplitterDrag::Explorer)
        );

        // Inside a pane (on no splitter at all): nothing arms.
        assert_eq!(resolve_splitter_drag(&state, size, 2, body_top + 1), None);
    }

    /// Every row of the app-level vertical splitter resolves to `App` —
    /// including the row the Explorer's horizontal splitter occupies. The two
    /// own different axes, so a vertical drag must never reach a height split.
    #[test]
    fn vertical_splitter_rows_never_resolve_to_a_height_split() {
        use crate::features::explorer::state::ExplorerPane;

        let mut state = AppState::default();
        state.term_width = 120;
        state.term_height = 40;
        state.focus = Pane::Explorer(ExplorerPane::Instances);
        let size = ratatui::layout::Size::new(120, 40);
        let (body_top, body_h) = test_body(&state, size);
        let layout = crate::features::app_splitter::view::app_body_layout(
            Rect::new(0, body_top, size.width, body_h),
            state.splitter.explorer_pane_width,
        );
        for dy in 0..layout.v_splitter.height {
            assert_eq!(
                resolve_splitter_drag(&state, size, layout.v_splitter.x, layout.v_splitter.y + dy),
                Some(SplitterDrag::App),
                "row {dy} of the vertical splitter must not resolve to another split"
            );
        }
    }

    /// With the SQL workspace focused, a press on the editor/history splitter
    /// arms that splitter alone — not the app-level one — so a drag there cannot
    /// resize the Explorer width (the reported "horizontal drag moves the
    /// Explorer" symptom).
    #[test]
    fn sql_tab_splitter_arms_without_the_app_splitter() {
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;

        let mut state = AppState::default();
        state.term_width = 120;
        state.term_height = 40;
        state.focus = Pane::SQLWorkspace;
        state.sql.sql_tab.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        state.sql.sql_tab.tabs[0].focus = SqlFocus::Editor;
        let size = ratatui::layout::Size::new(120, 40);
        let tab_area = sql_tab_area_for_hit(size, &state).expect("sql tab area");
        let body = Rect::new(
            tab_area.x,
            tab_area.y.saturating_add(1),
            tab_area.width,
            tab_area.height.saturating_sub(1),
        );
        let tab = &state.sql.sql_tab.tabs[0];
        let layout = crate::features::sql_workspace::sql_tab::layout::sql_tab_layout(
            body,
            tab.splitter.editor_top_height,
            tab.splitter.history_pane_width,
        );

        let got = resolve_splitter_drag(&state, size, layout.v_splitter.x, layout.v_splitter.y + 1);
        assert!(
            matches!(got, Some(SplitterDrag::Sql(_, _))),
            "a press on the SQL editor/history splitter must arm it, got {got:?}"
        );
        assert_ne!(
            got,
            Some(SplitterDrag::App),
            "the app-level width splitter must not arm alongside it"
        );
    }

    #[test]
    fn wheel_axis_maps_shift_and_native_horizontal_ticks() {
        use crossterm::event::{KeyModifiers, MouseEventKind};

        // A bare vertical wheel stays on the vertical axis.
        assert_eq!(
            wheel_axis(MouseEventKind::ScrollUp, KeyModifiers::NONE),
            (false, -1)
        );
        assert_eq!(
            wheel_axis(MouseEventKind::ScrollDown, KeyModifiers::NONE),
            (false, 1)
        );

        // Shift reinterprets a vertical tick as horizontal.
        assert_eq!(
            wheel_axis(MouseEventKind::ScrollUp, KeyModifiers::SHIFT),
            (true, -1)
        );
        assert_eq!(
            wheel_axis(MouseEventKind::ScrollDown, KeyModifiers::SHIFT),
            (true, 1)
        );

        // Terminals that report the gesture natively are horizontal regardless
        // of modifiers.
        assert_eq!(
            wheel_axis(MouseEventKind::ScrollLeft, KeyModifiers::NONE),
            (true, -1)
        );
        assert_eq!(
            wheel_axis(MouseEventKind::ScrollRight, KeyModifiers::NONE),
            (true, 1)
        );

        // Other modifiers must not hijack the axis.
        assert_eq!(
            wheel_axis(MouseEventKind::ScrollDown, KeyModifiers::CONTROL),
            (false, 1)
        );
    }

    #[test]
    fn explorer_child_areas_stack_trees() {
        let explorer = Rect::new(0, 3, 40, 21);
        let (instances, objects) = explorer_child_areas(explorer, 9);
        // Outer border: inner is (1,4,38,19); instances height 9 (in the
        // [20%,80%] range of the 19-row track).
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
        let (instances, _objects) = explorer_child_areas(Rect::new(0, 3, 1, 20), 9);
        let boundary = instances.y.saturating_add(instances.height);
        assert_eq!(
            explorer_pane_for_click(5, 3, 20, 9),
            ExplorerPane::Instances
        );
        assert_eq!(
            explorer_pane_for_click(boundary, 3, 20, 9),
            ExplorerPane::Instances
        );
        assert_eq!(
            explorer_pane_for_click(boundary.saturating_add(1), 3, 20, 9),
            ExplorerPane::Objects
        );
        assert_eq!(explorer_pane_for_click(21, 3, 20, 9), ExplorerPane::Objects);
    }

    #[test]
    fn explorer_marker_click_toggles_without_moving_the_cursor() {
        use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
        use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};

        // One instance row (cursor on it) so the marker click hits an instance.
        let mut state = AppState::default();
        state
            .explorer
            .instances
            .set_instances(vec![dbm_store::ManagedInstance {
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
        let msgs =
            explorer_row_click_msgs(ratatui::layout::Size::new(100, 50), 3, 50, 3, 5, &state)
                .expect("marker click maps to a row");
        let has_toggle = msgs.iter().any(|m| {
            matches!(
                m,
                AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
                    InstancesMsg::Message(InstancesMessage::ToggleExpandAt { row: 0 })
                )))
            )
        });
        assert!(has_toggle, "marker click toggles expansion: {msgs:?}");
        let has_jump = msgs.iter().any(|m| {
            matches!(
                m,
                AppMsg::Explorer(ExplorerMsg::Message(ExplorerMessage::Instances(
                    InstancesMsg::Message(InstancesMessage::JumpTo { .. })
                )))
            )
        });
        assert!(!has_jump, "marker click must not move the cursor: {msgs:?}");
    }

    #[test]
    fn is_explorer_toggle_click_detects_the_marker_column() {
        // Same layout as the marker test: size 100x50, explorer_w=20, body_top=3,
        // instances_area = Rect(1,4,...). Instance marker = x=3, row0 y=5.
        let mut state = AppState::default();
        state
            .explorer
            .instances
            .set_instances(vec![dbm_store::ManagedInstance {
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
        state
            .explorer
            .instances
            .set_instances(vec![dbm_store::ManagedInstance {
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
        let msgs =
            explorer_row_click_msgs(ratatui::layout::Size::new(100, 50), 3, 45, 3, 5, &state)
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
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        // Render the real explorer at the same geometry the click handler uses,
        // then confirm the rendered first instance row is at the y that maps to
        // row 0. This catches any render/row_at drift for the restart scenario
        // (A collapsed-unloaded, active B).
        let mut state = AppState::default();
        state
            .explorer
            .instances
            .set_instances(vec![dbm_store::ManagedInstance {
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
        let theme = crate::common::view::theme::default();
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
                    false,
                    false,
                    None,
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
                first_y = Some(y);
                break;
            }
        }
        let y = first_y.expect("first instance row rendered");
        // The click handler maps this rendered y (with marker x=3) to row 0.
        let msgs =
            explorer_row_click_msgs(ratatui::layout::Size::new(100, 50), 3, 45, 3, y, &state)
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
        // Fixed layout: width 100, explorer 20 -> workspace (20,3,80,50).
        // popup = 75% centered = (30,9,60,37); inner (31,10,58,35).
        // Engine height 3 -> body starts at y=13; footer is 1 row (the discover
        // hint line is always present) -> body spans [13,44).
        // Default targets_height 10 -> targets [13,23), results [23,44).
        let workspace = Rect::new(20, 3, 80, 50);
        let state = crate::features::discover::state::DiscoverState::default();
        let click = |col: u16, row: u16| discover_subpane_for_click(col, row, workspace, &state);
        // Engine: rows [popup.y, body.y) = [9, 13).
        assert_eq!(click(40, 11), Some(DiscoverPane::Engine));
        assert_eq!(click(40, 12), Some(DiscoverPane::Engine));
        // Targets: rows [13, 23).
        assert_eq!(click(40, 15), Some(DiscoverPane::Targets));
        assert_eq!(click(40, 22), Some(DiscoverPane::Targets));
        // Results: rows [23, 44).
        assert_eq!(click(40, 24), Some(DiscoverPane::Results));
        assert_eq!(click(40, 43), Some(DiscoverPane::Results));
        // Outside the popup: header row, explorer column, or beyond the popup
        // yields None.
        assert_eq!(click(10, 11), None); // explorer column
        assert_eq!(click(40, 1), None); // header row
        assert_eq!(click(99, 11), None); // beyond popup right edge
        assert_eq!(click(40, 44), None); // beyond body bottom edge (footer)
    }
}
