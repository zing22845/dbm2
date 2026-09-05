//! Pane navigation and pane-jump key bindings (Ctrl+h/j/k/l, uppercase
//! `S`/`I`/`O`/`H`/`R`, explorer-width `[`/`]` nudges).
//!
//! Part of the split keyboard layer; the entry points are
//! re-exported from the parent [`super`] module.

use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app_shell::msg::ShellMsg;
use crate::app_shell::pane::Pane;
use crate::features::explorer::state::ExplorerPane;
use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
use crate::features::sql_workspace::sql_tab::state::SqlFocus;
use crate::features::sql_workspace::state::SqlState;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
/// Nudge the Explorer pane width with `[` (shrink) / `]` (grow), matching the
/// splitter convention that the left-side pane (Explorer) is grown by `]`.
/// Returns `None` when the key is not a bare `[` / `]`.
pub(super) fn explorer_width_nudge(key: KeyEvent, state: &AppState) -> Option<AppMsg> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return None;
    }
    let nudge = match key.code {
        KeyCode::Char('[') => Some(crate::common::view::splitter::VerticalSplitterNudge::Left),
        KeyCode::Char(']') => Some(crate::common::view::splitter::VerticalSplitterNudge::Right),
        _ => None,
    };
    let delta = crate::common::view::splitter::width_delta_for_left_pane(
        nudge?,
        crate::common::view::splitter::WIDTH_NUDGE_STEP,
    );
    let next = (state.splitter.explorer_pane_width as i16 + delta).max(0) as u16;
    Some(AppMsg::SetExplorerWidth(next))
}

/// Move the focus pane one step in `dir`. The explorer is a parent pane whose
/// child sub-pane (instances / objects) is the focused region, so Ctrl+nav
/// cycles within it and crosses to its neighbors (header above, workspace to
/// the right), mirroring the `Discover` parent pane. Workspace/instance leave
/// left to the explorer and up to the header.
///
/// `instance_open` tells whether an instance workspace is currently shown in
/// the workspace region (driven by the explorer tree's active-workspace marker).
/// Moving right from the explorer must land on the *displayed* workspace: the
/// instance workspace when an instance is open, otherwise the SQL workspace —
/// otherwise the focus (SQLWorkspace) no longer matches what is on screen, and
/// Ctrl+nav inside the instance workspace stops working.
pub(super) fn switch_pane_by_dir(
    dir: crate::app_shell::nav::PaneDir,
    focus: Pane,
    instance_open: bool,
    explorer_pane: ExplorerPane,
) -> Option<AppMsg> {
    use crate::app_shell::nav::{ExplorerPane, IwPane, PaneDir as D};
    let pane = match (focus, dir) {
        // Header moves down into the explorer, restoring its last sub-pane.
        (Pane::Header, D::Down) => Pane::Explorer(explorer_pane),
        // Inside the explorer, Up/Down cycle instances <-> objects; Up from
        // instances leaves to the header.
        (Pane::Explorer(ExplorerPane::Instances), D::Up) => Pane::Header,
        (Pane::Explorer(ExplorerPane::Instances), D::Down) => Pane::Explorer(ExplorerPane::Objects),
        (Pane::Explorer(ExplorerPane::Objects), D::Up) => Pane::Explorer(ExplorerPane::Instances),
        // Explorer moves right into the displayed workspace (instance if open).
        (Pane::Explorer(_), D::Right) if instance_open => Pane::InstanceWorkspace(IwPane::Overview),
        (Pane::Explorer(_), D::Right) => Pane::SQLWorkspace,
        // Inside the instance workspace, Left/Right move overview <-> connections
        // (matching the original dbm's manager panes): Connections left ->
        // Overview, Overview left -> explorer (leave), Overview right ->
        // Connections, Connections right stays (no wrap).
        (Pane::InstanceWorkspace(IwPane::Connections), D::Left) => {
            Pane::InstanceWorkspace(IwPane::Overview)
        }
        (Pane::InstanceWorkspace(IwPane::Overview), D::Left) => Pane::Explorer(explorer_pane),
        (Pane::InstanceWorkspace(IwPane::Overview), D::Right) => {
            Pane::InstanceWorkspace(IwPane::Connections)
        }
        (Pane::InstanceWorkspace(IwPane::Connections), D::Right) => {
            Pane::InstanceWorkspace(IwPane::Connections)
        }
        // Workspace leaves left to the explorer (restoring its sub-pane) and up
        // to the header.
        (Pane::SQLWorkspace, D::Left) => Pane::Explorer(explorer_pane),
        (Pane::SQLWorkspace, D::Up) => Pane::Header,
        _ => return None,
    };
    tracing::debug!(from = ?focus, to = ?pane, "pane switch via Ctrl+nav");
    Some(AppMsg::Shell(ShellMsg::FocusChanged { pane }))
}

/// Uppercase-letter pane jump, mirroring the original dbm's `[S]`/`[I]`/`[O]`/
/// `[H]`/`[R]` title shortcuts:
/// - `S` → SQL editor, `H` → History, `R` → Results (workspace sub-panes);
/// - `I` → Instances, `O` → Objects (explorer sub-panes).
///
/// Matching the original dbm, these fire only for an *uppercase* letter (Shift
/// or Caps Lock) with no Ctrl/Alt/Meta, and are suppressed while typing in the
/// SQL editor (insert mode) so `S`/`H`/`R` do not interrupt a query mid-edit.
pub(super) fn pane_jump_from_key(key: KeyEvent, state: &AppState) -> Option<AppMsg> {
    use crate::common::utils::shortcuts::{
        caps_lock_active, effective_ascii_letter, pane_jump_modifiers_ok,
    };
    if !pane_jump_modifiers_ok(key.modifiers) {
        return None;
    }
    let KeyCode::Char(c) = key.code else {
        return None;
    };
    let upper = effective_ascii_letter(
        c,
        key.modifiers.contains(KeyModifiers::SHIFT),
        caps_lock_active(&key, false),
    );
    if !upper.is_ascii_uppercase() {
        return None;
    }
    // Suppress letter jumps while the workspace is "typing" — mirroring the
    // original dbm's `workspace_text_input_active`: the SQL editor (insert
    // mode), any sub-pane's active `/` search, or the context picker's search.
    // A search counts even when its input ended (Enter) but the filter is
    // still applied and visible on the pane's bottom border, so `S`/`H`/`R`
    // never jump away while a filter is shown.
    if let Pane::SQLWorkspace = state.focus
        && let Some(tab) = state
            .sql
            .sql_tab
            .active_tab
            .and_then(|i| state.sql.sql_tab.tabs.get(i))
    {
        let editor_insert = tab.focus == SqlFocus::Editor
            && matches!(tab.editor.editor.mode, edtui::EditorMode::Insert);
        let any_search_active = tab.editor.sql_search.search.is_visible()
            || tab.results.list.search.is_visible()
            || tab.history.list.search.is_visible()
            || (tab.focus == SqlFocus::Editor && tab.editor.context_picker.open);
        if editor_insert || any_search_active {
            return None;
        }
    }
    match upper {
        // Workspace sub-panes.
        'S' => Some(focus_subpane(SqlFocus::Editor)),
        'H' => Some(focus_subpane(SqlFocus::History)),
        'R' => Some(focus_subpane(SqlFocus::Results)),
        // Explorer sub-panes.
        'I' => Some(focus_explorer(
            crate::app_shell::nav::ExplorerPane::Instances,
        )),
        'O' => Some(focus_explorer(crate::app_shell::nav::ExplorerPane::Objects)),
        _ => None,
    }
}

/// Build a `SqlTabMessage::Focus` app message for the given workspace sub-pane.
pub(super) fn focus_subpane(focus: SqlFocus) -> AppMsg {
    AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
        SqlTabMessage::Focus(focus),
    ))))
}

/// Build a `ShellMsg::FocusChanged` app message moving focus to an explorer
/// sub-pane (instances / objects).
fn focus_explorer(sub: crate::app_shell::nav::ExplorerPane) -> AppMsg {
    AppMsg::Shell(ShellMsg::FocusChanged {
        pane: Pane::Explorer(sub),
    })
}

/// Move the active tab's sub-pane focus one step in `dir`, mirroring the
/// original dbm's `workspace_neighbor`:
/// - editor → right: history; editor → down: results;
/// - history → left: editor; history → down: results;
/// - results → up: the previous editor/history pane (`upper_pane`).
///
/// Returns `None` when the move would leave the workspace (e.g. editor → left,
/// which goes to the explorer; results/history → up/left boundaries).
pub(super) fn switch_subpane(
    dir: crate::app_shell::nav::PaneDir,
    sql: &SqlState,
) -> Option<AppMsg> {
    use crate::app_shell::nav::PaneDir;
    use crate::features::sql_workspace::sql_tab::state::SqlFocus;

    let tab = sql
        .sql_tab
        .active_tab
        .and_then(|i| sql.sql_tab.tabs.get(i))?;
    let focus = match (tab.focus, dir) {
        (SqlFocus::Editor, PaneDir::Right) => SqlFocus::History,
        (SqlFocus::Editor, PaneDir::Down) => SqlFocus::Results,
        (SqlFocus::History, PaneDir::Left) => SqlFocus::Editor,
        (SqlFocus::History, PaneDir::Down) => SqlFocus::Results,
        // Up from results returns to the pane that was active before entering
        // results (editor or history), matching the original dbm.
        (SqlFocus::Results, PaneDir::Up) => {
            if tab.upper_pane == SqlFocus::History {
                SqlFocus::History
            } else {
                SqlFocus::Editor
            }
        }
        _ => return None,
    };
    Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
        SqlTabMsg::Message(SqlTabMessage::Focus(focus)),
    ))))
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::app_shell::nav::IwPane;

    use crate::app::key::key_to_msg;

    use crate::features::sql_workspace::sql_tab::state::SqlTabState;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    /// A `SqlState` with `count` tabs open. The default state opens no tabs, so
    /// we open `count` explicitly (for `count == 0`, an empty tab state).
    /// All tabs share the same connection so they are all visible.
    fn state_with_tabs(count: usize) -> SqlState {
        let mut tab_state = SqlTabState::default();
        for _ in 0..count {
            tab_state.open_connection_tab(
                "local".into(),
                "main-db".into(),
                "c1".into(),
                None,
                None,
                None,
            );
        }
        SqlState { sql_tab: tab_state }
    }

    fn extract_tab_msg(msg: AppMsg) -> SqlTabMessage {
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(m)))) => m,
            _ => panic!("expected Sql tab message"),
        }
    }
    #[test]
    fn ctrl_l_in_sql_workspace_moves_subpane_editor_to_history() {
        // Default tab focus is Editor; Ctrl+l (right) moves editor → history,
        // matching the original dbm's `workspace_neighbor`.
        let sql = state_with_tabs(1);
        let msg = switch_subpane(crate::app_shell::nav::PaneDir::Right, &sql)
            .expect("editor right should move to history");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Focus(SqlFocus::History)
        );
    }

    #[test]
    fn workspace_left_returns_to_explorer_remembered_subpane() {
        use crate::app_shell::nav::{ExplorerPane, PaneDir};
        // From the SQL workspace going Left, the explorer is restored at its
        // remembered sub-pane (Objects) rather than resetting to Instances.
        let msg = switch_pane_by_dir(
            PaneDir::Left,
            Pane::SQLWorkspace,
            false,
            ExplorerPane::Objects,
        )
        .expect("workspace left should return to the explorer");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged {
                pane: Pane::Explorer(sub),
            }) => {
                assert_eq!(sub, ExplorerPane::Objects)
            }
            other => panic!("expected explorer, got {other:?}"),
        }
    }

    #[test]
    fn ctrl_l_from_editor_does_not_leave_workspace() {
        // Editor → left leaves to the explorer (shell-level), so switch_subpane
        // returns None for it.
        let sql = state_with_tabs(1);
        assert!(
            switch_subpane(crate::app_shell::nav::PaneDir::Left, &sql).is_none(),
            "editor left must fall through to the explorer"
        );
    }

    #[test]
    fn ctrl_j_in_sql_workspace_from_editor_moves_to_results() {
        // Editor down → results (results sits below-right of the editor).
        let sql = state_with_tabs(1);
        let msg = switch_subpane(crate::app_shell::nav::PaneDir::Down, &sql)
            .expect("editor down should move to results");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Focus(SqlFocus::Results)
        );
    }

    #[test]
    fn ctrl_j_from_results_does_not_move() {
        // The original dbm has no Down neighbor for Results, so it returns None
        // (falls through to shell-level switching).
        let mut sql = state_with_tabs(1);
        sql.sql_tab.tabs[0].focus = SqlFocus::Results;
        assert!(
            switch_subpane(crate::app_shell::nav::PaneDir::Down, &sql).is_none(),
            "results down must not move within the workspace"
        );
    }

    #[test]
    fn ctrl_k_from_results_returns_to_upper_pane() {
        // Results → Up returns to the previous editor/history pane (upper_pane).
        let mut sql = state_with_tabs(1);
        sql.sql_tab.tabs[0].focus = SqlFocus::Results;
        sql.sql_tab.tabs[0].upper_pane = SqlFocus::History;
        let msg = switch_subpane(crate::app_shell::nav::PaneDir::Up, &sql)
            .expect("results up should return to history");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Focus(SqlFocus::History)
        );
        // Default upper_pane is editor.
        sql.sql_tab.tabs[0].upper_pane = SqlFocus::Editor;
        let msg = switch_subpane(crate::app_shell::nav::PaneDir::Up, &sql)
            .expect("results up should return to editor");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Focus(SqlFocus::Editor));
    }

    #[test]
    fn uppercase_s_jumps_to_sql_editor() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::default()); // jump from another pane
        let msg = key_to_msg(key(KeyCode::Char('S'), KeyModifiers::SHIFT), &state)
            .expect("uppercase S should jump to the SQL editor");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Focus(f),
            )))) => assert_eq!(f, SqlFocus::Editor),
            other => panic!("expected Focus(Editor), got {other:?}"),
        }
    }

    #[test]
    fn uppercase_h_and_r_jump_to_history_and_results() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        let msg = key_to_msg(key(KeyCode::Char('H'), KeyModifiers::SHIFT), &state)
            .expect("uppercase H should jump to history");
        assert!(matches!(
            msg,
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Focus(f)
            )))) if f == SqlFocus::History
        ));
        let msg = key_to_msg(key(KeyCode::Char('R'), KeyModifiers::SHIFT), &state)
            .expect("uppercase R should jump to results");
        assert!(matches!(
            msg,
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Focus(f)
            )))) if f == SqlFocus::Results
        ));
    }

    #[test]
    fn uppercase_i_and_o_jump_to_explorer_panes() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        let msg = key_to_msg(key(KeyCode::Char('I'), KeyModifiers::SHIFT), &state)
            .expect("uppercase I should jump to instances");
        assert!(matches!(
            msg,
            AppMsg::Shell(ShellMsg::FocusChanged {
                pane: Pane::Explorer(ExplorerPane::Instances)
            })
        ));
        let msg = key_to_msg(key(KeyCode::Char('O'), KeyModifiers::SHIFT), &state)
            .expect("uppercase O should jump to objects");
        assert!(matches!(
            msg,
            AppMsg::Shell(ShellMsg::FocusChanged {
                pane: Pane::Explorer(ExplorerPane::Objects)
            })
        ));
    }

    #[test]
    fn jump_suppressed_while_typing_in_sql_editor_insert_mode() {
        let mut state = crate::app::state::AppState::default();
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
        state.sql.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        // While typing (insert mode), uppercase S must NOT jump away (it becomes
        // a normal keystroke), so the result must not be a Focus/pane-jump msg.
        let msg = key_to_msg(key(KeyCode::Char('S'), KeyModifiers::SHIFT), &state);
        assert!(
            !matches!(
                msg,
                Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::Focus(_))
                ))))
            ),
            "S while typing in the editor must not jump to another pane"
        );
    }

    #[test]
    fn lowercase_letters_do_not_jump() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        // Plain lowercase s/i/o/h/r (no Shift/Caps) must not trigger a jump.
        for c in ['s', 'i', 'o', 'h', 'r'] {
            assert!(
                key_to_msg(key(KeyCode::Char(c), KeyModifiers::NONE), &state).is_none(),
                "plain lowercase {c} must not jump"
            );
        }
    }

    #[test]
    fn ctrl_j_moves_from_header_to_explorer() {
        let state = crate::app::state::AppState::default();
        assert_eq!(state.focus, Pane::Header);
        let msg = key_to_msg(key(KeyCode::Char('j'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+j should switch pane");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::Explorer(ExplorerPane::default()));
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_h_moves_from_workspace_back_to_explorer() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        let msg = key_to_msg(key(KeyCode::Char('h'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+h should switch pane");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::Explorer(ExplorerPane::default()));
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_l_moves_from_explorer_to_workspace() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::default());
        let msg = key_to_msg(key(KeyCode::Char('l'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+l should switch pane");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::SQLWorkspace);
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_j_within_explorer_switches_instances_to_objects() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::Instances);
        let msg = key_to_msg(key(KeyCode::Char('j'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+j inside explorer should switch sub-pane");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::Explorer(ExplorerPane::Objects));
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_k_within_explorer_switches_objects_to_instances() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::Objects);
        let msg = key_to_msg(key(KeyCode::Char('k'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+k inside explorer should switch sub-pane");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::Explorer(ExplorerPane::Instances));
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_l_in_instance_workspace_moves_overview_to_connections() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Overview);
        let msg = key_to_msg(key(KeyCode::Char('l'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+l inside instance workspace should move to connections");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::InstanceWorkspace(IwPane::Connections));
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_l_from_explorer_enters_instance_workspace_when_instance_open() {
        use crate::features::instance_workspace::state::IwState;

        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::default());
        // An instance is active, so the workspace region shows the instance pane.
        state.explorer.instances.set_active_instance(0);
        state.iw = IwState {
            instance_name: "inst".into(),
            ..Default::default()
        };
        let msg = key_to_msg(key(KeyCode::Char('l'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+l should move into the instance workspace");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::InstanceWorkspace(IwPane::Overview));
            }
            _ => panic!("expected focus change to instance workspace"),
        }

        // From the instance overview, ctrl+l moves to Connections.
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Overview);
        let msg = key_to_msg(key(KeyCode::Char('l'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+l inside overview should move to connections");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::InstanceWorkspace(IwPane::Connections));
            }
            _ => panic!("expected focus change to connections"),
        }
    }

    #[test]
    fn ctrl_h_in_instance_workspace_moves_connections_to_overview() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Connections);
        let msg = key_to_msg(key(KeyCode::Char('h'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+h inside instance workspace should move to overview");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::InstanceWorkspace(IwPane::Overview));
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_h_in_instance_workspace_overview_leaves_to_explorer() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Overview);
        let msg = key_to_msg(key(KeyCode::Char('h'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+h from overview should leave to explorer");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::Explorer(ExplorerPane::default()));
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn ctrl_k_from_explorer_instances_leaves_to_header() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::Instances);
        let msg = key_to_msg(key(KeyCode::Char('k'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+k from explorer instances should leave to header");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::Header);
            }
            _ => panic!("expected focus change"),
        }
    }

    #[test]
    fn bracket_keys_nudge_explorer_width_outside_workspace() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::Explorer(ExplorerPane::Instances);
        state.splitter.explorer_pane_width = 24;
        // `]` grows the Explorer pane (left side); `[` shrinks it.
        let msg = key_to_msg(key(KeyCode::Char(']'), KeyModifiers::NONE), &state)
            .expect("] should nudge the explorer wider");
        assert!(matches!(msg, AppMsg::SetExplorerWidth(w) if w == 26));
        let msg = key_to_msg(key(KeyCode::Char('['), KeyModifiers::NONE), &state)
            .expect("[ should nudge the explorer narrower");
        assert!(matches!(msg, AppMsg::SetExplorerWidth(w) if w == 22));
    }

    #[test]
    fn bracket_keys_do_not_nudge_explorer_inside_sql_workspace() {
        // Inside the SQL workspace `[`/`]` resize the history/detail splitters
        // (handled by `sql_key`), not the Explorer pane.
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        state.splitter.explorer_pane_width = 24;
        let msg = key_to_msg(key(KeyCode::Char(']'), KeyModifiers::NONE), &state);
        // Not an app-level SetExplorerWidth (the key is consumed by sql_key).
        assert!(!matches!(msg, Some(AppMsg::SetExplorerWidth(_))));
    }

    #[test]
    fn ctrl_h_from_workspace_leaves_to_explorer_even_when_results_focused() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        // Put the active tab on Results so the old `switch_subpane` path would
        // have intercepted ctrl+h; top-level nav must still win.
        if let Some(tab) = state
            .sql
            .sql_tab
            .active_tab
            .and_then(|i| state.sql.sql_tab.tabs.get_mut(i))
        {
            tab.focus = crate::features::sql_workspace::sql_tab::state::SqlFocus::Results;
        }
        let msg = key_to_msg(key(KeyCode::Char('h'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+h from workspace should leave to explorer");
        match msg {
            AppMsg::Shell(ShellMsg::FocusChanged { pane }) => {
                assert_eq!(pane, Pane::Explorer(ExplorerPane::default()));
            }
            _ => panic!("expected focus change"),
        }
    }
}
