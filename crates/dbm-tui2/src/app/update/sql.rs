//! SQL workspace messages: forwarded to the feature's update.

use super::UpdateResult;
use super::{box_effect, box_intent, sync_objects_active, sync_objects_binding};
use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app_shell::pane::Pane;
use crate::features::explorer::effect::ExplorerEffect;
use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
use crate::features::sql_workspace::update::update as sql_workspace_update;

pub(super) fn apply(msg: AppMsg, state: &mut AppState, result: &mut UpdateResult) {
    let AppMsg::Sql(m) = msg else {
        return;
    };
    let SqlMsg::Message(inner) = m;
    // A `SqlTabMessage::Focus` (from the uppercase S/H/R pane-jump
    // shortcuts) also moves the shell focus into the SQL workspace, so
    // jumping from the explorer/header lands on the editor/results/
    // history sub-pane rather than leaving the shell focus behind.
    //
    // Only mark dirty when the shell focus *actually moves*. Clicking
    // an already-focused sub-pane (e.g. the detail inside History while
    // the workspace owns focus) sends a Focus message that changes
    // nothing; unconditionally dirtying would render an identical frame
    // (changed_cells == 0) and pollute the redundancy metric.
    if matches!(
        inner,
        SqlMessage::SqlTab(SqlTabMsg::Message(SqlTabMessage::Focus(_)))
    ) && state.focus != Pane::SQLWorkspace
    {
        state.focus = Pane::SQLWorkspace;
        result.dirty = true;
    }
    // The sql feature's update is a pure by-value transition: move the
    // state out, update it, move the result back. No deep clone.
    let sql = std::mem::take(&mut state.sql);
    let (s, intents, effects, d) = sql_workspace_update(inner, sql);
    state.sql = s;
    result.dirty |= d;
    result.intents.extend(intents.into_iter().map(box_intent));
    result.effects.extend(effects.into_iter().map(box_effect));
    // Keep the objects tree's binding + active schema in sync with the
    // active SQL tab / connection. The active path is forced expanded
    // and cannot be collapsed (original dbm).
    if let Some(bind) = sync_objects_binding(&mut state.explorer.objects, &state.explorer.instances)
    {
        result.pending.push_back(bind);
    }
    if let Some(effect) = sync_objects_active(&mut state.explorer.objects, &state.sql) {
        result
            .effects
            .push(box_effect(ExplorerEffect::Objects(effect)));
    }
}

#[cfg(test)]
mod tests {
    use super::super::update_unchecked;
    use super::*;

    fn focus_changed_msg(pane: Pane) -> AppMsg {
        AppMsg::Shell(crate::app_shell::msg::ShellMsg::FocusChanged { pane })
    }

    #[test]
    fn sql_subpane_focus_survives_round_trip_to_explorer() {
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;
        let mut state = AppState::default();
        state.focus = Pane::SQLWorkspace;
        // Open a tab and focus History inside the workspace.
        state.sql.sql_tab.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        state.sql.sql_tab.tabs[0].focus = SqlFocus::History;

        // Leave to the explorer, then come back to the SQL workspace.
        update_unchecked(
            focus_changed_msg(Pane::Explorer(
                crate::app_shell::nav::ExplorerPane::default(),
            )),
            &mut state,
        );
        update_unchecked(focus_changed_msg(Pane::SQLWorkspace), &mut state);

        // The sub-pane focus is remembered per tab (shell FocusChanged only
        // moves `state.focus`, never `tab.focus`), so History is still active.
        assert_eq!(state.focus, Pane::SQLWorkspace);
        assert_eq!(
            state.sql.sql_tab.tabs[0].focus,
            SqlFocus::History,
            "sub-pane focus must survive leaving and re-entering the workspace"
        );
    }
    #[test]
    fn closing_all_tabs_leaves_sql_tab_empty() {
        use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
        use crate::features::sql_workspace::sql_tab::msg::SqlTabMessage;

        let mut state = AppState::default();
        // Open one tab for the active connection.
        state.sql.sql_tab.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "c1-id".into(),
            None,
            None,
            None,
        );
        assert_eq!(state.sql.sql_tab.tabs.len(), 1);
        assert_eq!(state.sql.sql_tab.visible_tab_count(), 1);

        let close_tab_msg = |visible: usize| {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                crate::features::sql_workspace::sql_tab::msg::SqlTabMsg::Message(
                    SqlTabMessage::CloseTab(visible),
                ),
            )))
        };

        // Close the only visible tab (offset 0).
        let r = update_unchecked(close_tab_msg(0), &mut state);
        assert!(r.dirty, "closing the last tab must mark the view dirty");
        assert!(state.sql.sql_tab.tabs.is_empty(), "all tabs must be closed");
        assert_eq!(state.sql.sql_tab.visible_tab_count(), 0);
        // `sql_tab/view.rs` renders the empty-state hint when tabs are empty.
        assert!(state.sql.sql_tab.tabs.is_empty());
    }
}
