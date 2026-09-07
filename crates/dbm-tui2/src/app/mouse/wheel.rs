//! Scroll-wheel handling, one function per pane that scrolls.
//!
//! Every handler resolves the wheel delta for its pane and dispatches the
//! scroll message; the shared trackpad debounce lives in
//! [`debounced`](fn@debounced) below.

use std::time::Instant;

use crossterm::event::{MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Size};
use tokio::sync::mpsc;

use crate::app::action::Action;
use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app_shell::effect::EffectRunner;

use super::click::explorer_child_areas;
use crate::app::geometry::{
    app_explorer_rect, iw_body_area_for_hit, sql_tab_area_for_hit, workspace_rect_for_hit,
};
use crate::app::round::process_message_round;

/// Trackpad wheel debounce window: a macOS trackpad emits a burst of ticks per
/// physical gesture, so ticks closer together than this collapse into one.
const WHEEL_DEBOUNCE_MS: u128 = 15;

/// Columns scrolled per horizontal wheel tick.
///
/// Horizontal content is typically far wider than it is tall, so reusing the
/// vertical wheel's one-cell-per-tick granularity would make a wide table
/// impractical to traverse. Three columns per tick keeps a gesture useful while
/// staying fine-grained enough to land on a column.
const WHEEL_H_STEP: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WheelOutcome {
    /// The tick was applied to the focused pane.
    Handled,
    /// The tick was a duplicate from a trackpad burst: ignore it and skip the
    /// rest of the loop iteration (no repaint).
    Debounced,
}

/// Wheel over the discover targets editor: vertical paging.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_wheel_discover_targets(
    mouse: &MouseEvent,
    size: Size,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    point: Position,
    dirty: &mut bool,
    last_wheel: &mut Option<(Instant, i32, bool)>,
    idle_iterations: &mut u32,
) -> anyhow::Result<WheelOutcome> {
    // Wheel debounce: collapse macOS trackpad burst events
    // so each physical tick maps to one MoveUp/Down.
    // Shift+wheel is the platform convention for
    // horizontal scrolling; some terminals instead
    // report the gesture natively as ScrollLeft/Right.
    let (horizontal, dir) = wheel_axis(mouse.kind, mouse.modifiers);
    let now = std::time::Instant::now();
    if let Some((t, d, h)) = *last_wheel
        && d == dir
        && h == horizontal
        && now.duration_since(t).as_millis() < WHEEL_DEBOUNCE_MS
    {
        *idle_iterations = 0;
        return Ok(WheelOutcome::Debounced);
    }
    *last_wheel = Some((now, dir, horizontal));

    let footer_h = crate::app::geometry::global_footer_height(state, size.width);
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    if let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state)
        && let discover_popup = crate::common::layout::modal::popup_rect(workspace, 75, 75)
        && let body =
            crate::features::discover::layout::discover_body_area(discover_popup, &state.discover)
        && !body.is_empty()
        && let layout = crate::features::discover::splitter::layout::discover_body_layout(
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
                    crate::features::discover::msg::DiscoverMessage::Targets(TargetsMsg::Message(
                        TargetsMessage::MoveUp,
                    )),
                ))
            }
            MouseEventKind::ScrollDown => {
                AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                    crate::features::discover::msg::DiscoverMessage::Targets(TargetsMsg::Message(
                        TargetsMessage::MoveDown,
                    )),
                ))
            }
            _ => unreachable!(),
        };
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }
    Ok(WheelOutcome::Handled)
}

/// Wheel over the discover results list: horizontal paging.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_wheel_discover_results(
    mouse: &MouseEvent,
    size: Size,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    dirty: &mut bool,
    last_wheel: &mut Option<(Instant, i32, bool)>,
    idle_iterations: &mut u32,
) -> anyhow::Result<WheelOutcome> {
    // Shift+wheel is the platform convention for
    // horizontal scrolling; some terminals instead
    // report the gesture natively as ScrollLeft/Right.
    let (horizontal, dir) = wheel_axis(mouse.kind, mouse.modifiers);
    let now = std::time::Instant::now();
    if let Some((t, d, h)) = *last_wheel
        && d == dir
        && h == horizontal
        && now.duration_since(t).as_millis() < WHEEL_DEBOUNCE_MS
    {
        *idle_iterations = 0;
        return Ok(WheelOutcome::Debounced);
    }
    *last_wheel = Some((now, dir, horizontal));

    let footer_h = crate::app::geometry::global_footer_height(state, size.width);
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    let point = ratatui::layout::Position::new(mouse.column, mouse.row);
    if let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state)
        && let discover_popup = crate::common::layout::modal::popup_rect(workspace, 75, 75)
        && let body =
            crate::features::discover::layout::discover_body_area(discover_popup, &state.discover)
        && !body.is_empty()
        && let layout = crate::features::discover::splitter::layout::discover_body_layout(
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
                    crate::features::discover::msg::DiscoverMessage::Results(ResultsMsg::Message(
                        ResultsMessage::MoveUp,
                    )),
                ))
            }
            MouseEventKind::ScrollDown => {
                AppMsg::Discover(crate::features::discover::msg::DiscoverMsg::Message(
                    crate::features::discover::msg::DiscoverMessage::Results(ResultsMsg::Message(
                        ResultsMessage::MoveDown,
                    )),
                ))
            }
            _ => unreachable!(),
        };
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }
    Ok(WheelOutcome::Handled)
}

/// Wheel over the Explorer objects tree.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_wheel_explorer_objects(
    mouse: &MouseEvent,
    size: Size,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    dirty: &mut bool,
    last_wheel: &mut Option<(Instant, i32, bool)>,
    idle_iterations: &mut u32,
) -> anyhow::Result<WheelOutcome> {
    // Shift+wheel is the platform convention for
    // horizontal scrolling; some terminals instead
    // report the gesture natively as ScrollLeft/Right.
    let (horizontal, dir) = wheel_axis(mouse.kind, mouse.modifiers);
    let now = std::time::Instant::now();
    if let Some((t, d, h)) = *last_wheel
        && d == dir
        && h == horizontal
        && now.duration_since(t).as_millis() < WHEEL_DEBOUNCE_MS
    {
        *idle_iterations = 0;
        return Ok(WheelOutcome::Debounced);
    }
    *last_wheel = Some((now, dir, horizontal));

    let footer_h = crate::app::geometry::global_footer_height(state, size.width);
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
        *dirty |= result.dirty;
    }
    Ok(WheelOutcome::Handled)
}

/// Wheel over the Explorer instances tree.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_wheel_explorer_instances(
    mouse: &MouseEvent,
    size: Size,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    dirty: &mut bool,
    last_wheel: &mut Option<(Instant, i32, bool)>,
    idle_iterations: &mut u32,
) -> anyhow::Result<WheelOutcome> {
    // Shift+wheel is the platform convention for
    // horizontal scrolling; some terminals instead
    // report the gesture natively as ScrollLeft/Right.
    let (horizontal, dir) = wheel_axis(mouse.kind, mouse.modifiers);
    let now = std::time::Instant::now();
    if let Some((t, d, h)) = *last_wheel
        && d == dir
        && h == horizontal
        && now.duration_since(t).as_millis() < WHEEL_DEBOUNCE_MS
    {
        *idle_iterations = 0;
        return Ok(WheelOutcome::Debounced);
    }
    *last_wheel = Some((now, dir, horizontal));

    let footer_h = crate::app::geometry::global_footer_height(state, size.width);
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
        use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
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
        *dirty |= result.dirty;
    }
    Ok(WheelOutcome::Handled)
}

/// Wheel over the Instance Workspace connections pane.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_wheel_iw_connections(
    mouse: &MouseEvent,
    size: Size,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    dirty: &mut bool,
    last_wheel: &mut Option<(Instant, i32, bool)>,
    idle_iterations: &mut u32,
) -> anyhow::Result<WheelOutcome> {
    // Shift+wheel is the platform convention for
    // horizontal scrolling; some terminals instead
    // report the gesture natively as ScrollLeft/Right.
    let (horizontal, dir) = wheel_axis(mouse.kind, mouse.modifiers);
    let now = std::time::Instant::now();
    if let Some((t, d, h)) = *last_wheel
        && d == dir
        && h == horizontal
        && now.duration_since(t).as_millis() < WHEEL_DEBOUNCE_MS
    {
        *idle_iterations = 0;
        return Ok(WheelOutcome::Debounced);
    }
    *last_wheel = Some((now, dir, horizontal));

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
            MouseEventKind::ScrollUp => AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
                ConnectionsMsg::Message(ConnectionsMessage::MoveUp),
            ))),
            MouseEventKind::ScrollDown => AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
                ConnectionsMsg::Message(ConnectionsMessage::MoveDown),
            ))),
            _ => unreachable!(),
        };
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }
    Ok(WheelOutcome::Handled)
}

/// Wheel over the Instance Workspace overview pane.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_wheel_iw_overview(
    mouse: &MouseEvent,
    size: Size,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    dirty: &mut bool,
    last_wheel: &mut Option<(Instant, i32, bool)>,
    idle_iterations: &mut u32,
) -> anyhow::Result<WheelOutcome> {
    // Shift+wheel is the platform convention for
    // horizontal scrolling; some terminals instead
    // report the gesture natively as ScrollLeft/Right.
    let (horizontal, dir) = wheel_axis(mouse.kind, mouse.modifiers);
    let now = std::time::Instant::now();
    if let Some((t, d, h)) = *last_wheel
        && d == dir
        && h == horizontal
        && now.duration_since(t).as_millis() < WHEEL_DEBOUNCE_MS
    {
        *idle_iterations = 0;
        return Ok(WheelOutcome::Debounced);
    }
    *last_wheel = Some((now, dir, horizontal));

    if let Some(body) = iw_body_area_for_hit(size, state)
        && let point = ratatui::layout::Position::new(mouse.column, mouse.row)
        && body.contains(point)
    {
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
        use crate::features::instance_workspace::overview::msg::{OverviewMessage, OverviewMsg};
        // No horizontal scroll in this pane, so a
        // shift-wheel falls through to the vertical axis.
        let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Overview(OverviewMsg::Message(
            OverviewMessage::MoveCursor(dir),
        ))));
        let result = process_message_round(effect_runner, action_rx, msg, state);
        *dirty |= result.dirty;
    }
    Ok(WheelOutcome::Handled)
}

/// Wheel inside the SQL workspace: routes to the focused sub-pane.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_wheel_sql(
    mouse: &MouseEvent,
    size: Size,
    state: &mut AppState,
    effect_runner: &EffectRunner<Action>,
    action_rx: &mut mpsc::UnboundedReceiver<Action>,
    point: Position,
    dirty: &mut bool,
    last_wheel: &mut Option<(Instant, i32, bool)>,
    idle_iterations: &mut u32,
) -> anyhow::Result<WheelOutcome> {
    // Wheel debounce — same as discover targets above.
    // Shift+wheel is the platform convention for
    // horizontal scrolling; some terminals instead
    // report the gesture natively as ScrollLeft/Right.
    let (horizontal, dir) = wheel_axis(mouse.kind, mouse.modifiers);
    let now = std::time::Instant::now();
    if let Some((t, d, h)) = *last_wheel
        && d == dir
        && h == horizontal
        && now.duration_since(t).as_millis() < WHEEL_DEBOUNCE_MS
    {
        *idle_iterations = 0;
        return Ok(WheelOutcome::Debounced);
    }
    *last_wheel = Some((now, dir, horizontal));

    if let Some(tab_area) = sql_tab_area_for_hit(size, state)
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
            use crate::features::sql_workspace::sql_tab::editor::msg::{EditorMessage, EditorMsg};
            use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
            // Shift+wheel scrolls the editor sideways;
            // a bare wheel keeps scrolling by lines.
            let delta: i32 = if horizontal {
                dir * WHEEL_H_STEP
            } else {
                dir * 3
            };
            let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    tab_id: tab_idx,
                    msg: EditorMsg::Message(if horizontal {
                        EditorMessage::ScrollH { delta }
                    } else {
                        EditorMessage::ScrollV { delta }
                    }),
                },
            ))));
            let result = process_message_round(effect_runner, action_rx, msg, state);
            *dirty |= result.dirty;
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
                use crate::features::sql_workspace::sql_tab::history::splitter::layout::{
                    history_zone_width, history_zone_x,
                };
                let zone_x =
                    history_zone_x(tab_area, &layout, tab.history.splitter.detail_pane_width);
                let zone_w = history_zone_width(&layout, tab.history.splitter.detail_pane_width);
                ratatui::layout::Rect::new(zone_x, layout.history.y, zone_w, layout.history.height)
            } else {
                layout.history
            };
            if zone_rect.contains(point) {
                use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
                use crate::features::sql_workspace::sql_tab::history::msg::{
                    HistoryMessage, HistoryMsg,
                };
                use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
                // Shift+wheel scrolls the history rows
                // sideways; a bare wheel moves the cursor.
                let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::History {
                        tab_id: tab_idx,
                        msg: HistoryMsg::Message(if horizontal {
                            HistoryMessage::ScrollHScroll {
                                delta: dir * WHEEL_H_STEP,
                            }
                        } else {
                            HistoryMessage::MoveCursor { delta: dir }
                        }),
                    },
                ))));
                let result = process_message_round(effect_runner, action_rx, msg, state);
                *dirty |= result.dirty;
            } else if layout.results.contains(point) {
                // Scroll wheel on the results pane: move
                // the cell selection up/down. The view
                // auto-adjusts v_scroll to keep cursor visible.
                use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
                use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
                use crate::features::sql_workspace::sql_tab::results::msg::{
                    ResultsMessage, ResultsMsg,
                };
                // Shift+wheel scrolls the grid sideways;
                // a bare wheel moves the cell selection.
                let msg = AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Results {
                        tab_id: tab_idx,
                        msg: ResultsMsg::Message(if horizontal {
                            ResultsMessage::ScrollHScroll {
                                delta: dir * WHEEL_H_STEP,
                            }
                        } else {
                            ResultsMessage::MoveSelection { dr: dir, dc: 0 }
                        }),
                    },
                ))));
                let result = process_message_round(effect_runner, action_rx, msg, state);
                *dirty |= result.dirty;
            }
        }
    }
    Ok(WheelOutcome::Handled)
}
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
