//! Click routing: turning a press position into app messages.
//!
//! Each function answers "which pane owns this cell, and what message does that
//! pane want" for one region of the screen. They are pure — no state is
//! mutated; the caller dispatches the returned messages through `update`.

use ratatui::layout::Rect;

use crate::app::msg::AppMsg;
use crate::app::state::AppState;

use super::geometry::app_explorer_rect;

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
