//! Tab management messages: switching, opening, closing and focusing
//! tabs, and applying a connection context to a tab.

use super::super::editor;
use super::super::effect::SqlTabEffect;
use super::super::history;
use super::super::msg::SqlTabMessage;
use super::super::state::{SqlFocus, SqlTabState};
use super::{SqlTabOut, clamp_list_for_history_detail, warn_tab_missing};

pub(super) fn apply(msg: SqlTabMessage, state: &mut SqlTabState, out: &mut SqlTabOut) {
    match msg {
        SqlTabMessage::Tab(visible_idx) => {
            // Map the visible-tab offset to a global index within the active
            // connection's tabs, mirroring the original dbm's `switch_tab_index`.
            let before = state.active_tab;
            if let Some(global) = state.visible_to_global(visible_idx) {
                state.active_tab = Some(global);
                // Clearing a leftover picker on the newly active tab prevents it
                // from blocking that tab's editor (keys / context clicks).
                if Some(global) != before {
                    state.close_active_context_picker();
                }
                // Keep the connection's "current tab" bookmark in sync with the
                // active tab, so a new tab opened later (`Alt+t`/`n`) inherits
                // the currently active tab's context rather than a stale one.
                state.remember_active_tab_for_connection();
            }
            out.dirty = before != state.active_tab;
        }
        SqlTabMessage::Focus(focus) => {
            if let Some(tab) = state.active_tab.and_then(|i| state.tabs.get_mut(i)) {
                // Leaving Results while it holds unsaved edits is gated like an
                // Esc leave: unsaved changes block the move so nothing is
                // dropped, and a clean detail editor quietly drops its focus.
                if focus != SqlFocus::Results && tab.focus == SqlFocus::Results {
                    let detail_blocks = tab.results.detail.focused && tab.results.detail.dirty;
                    let list_blocks =
                        tab.results.list.edit.editing && tab.results.list.edit.is_dirty();
                    if detail_blocks || list_blocks {
                        if detail_blocks {
                            tab.results.detail.leave_warning = true;
                        }
                        if list_blocks {
                            tab.results.list.leave_warning = true;
                        }
                        out.dirty = true;
                        return;
                    }
                    if tab.results.detail.focused {
                        tab.results.detail.unfocus();
                    }
                    tab.results.list.leave_warning = false;
                }
                let mut changed = tab.focus != focus;
                // Track the sub-pane that was active before entering Results, so
                // Ctrl+Up from Results returns to the previous editor/history
                // pane (mirroring the original dbm's `workspace_upper_pane`).
                if focus == SqlFocus::Results
                    && matches!(tab.focus, SqlFocus::Editor | SqlFocus::History)
                {
                    tab.upper_pane = tab.focus;
                }
                tab.focus = focus;
                // Close editor-owned popups when focus leaves the editor — they
                // are children of the editor pane and must not keep intercepting
                // keys while the user is interacting with history or results.
                if focus != SqlFocus::Editor {
                    if tab.editor.sql_completion.is_open() {
                        tab.editor.sql_completion.close();
                        changed = true;
                    }
                    if tab.editor.context_picker.open {
                        tab.editor.context_picker.close();
                        changed = true;
                    }
                }
                // Entering History makes the detail pane visible, which adds
                // `detail + splitter` columns to the History zone. A list width
                // that was legal while History was unfocused (up to
                // `history_max`) can then overflow `history_max - detail` and
                // squeeze the *rendered* list below its stored width. Re-clamp
                // so storage and rendered geometry stay identical.
                changed |= clamp_list_for_history_detail(tab, &state.history_store);
                out.dirty = changed;
            }
        }
        SqlTabMessage::OpenTab => {
            state.open_tab();
            out.dirty = true;
        }
        SqlTabMessage::CloseTab(visible_idx) => {
            // Map the visible offset to a global index before closing.
            if let Some(global) = state.visible_to_global(visible_idx) {
                state.close_tab(global);
                out.dirty = true;
            }
        }
        SqlTabMessage::OpenConnectionTab {
            instance,
            connection,
            connection_id,
            database,
            schema,
            default_database,
        } => {
            state.open_connection_tab(
                instance.clone(),
                connection.clone(),
                connection_id,
                database.clone(),
                schema.clone(),
                default_database.as_deref(),
            );
            // Load the SQL-completion catalog for the newly bound tab so table
            // and column completion is available immediately.
            let tab_id = state.tabs.last().map(|t| t.session.id).unwrap_or_default();
            let schema_name = schema.clone().unwrap_or_else(|| "public".to_string());
            out.effects.push(SqlTabEffect::Editor {
                tab_id,
                effect: editor::effect::EditorEffect::LoadCompletionCatalog {
                    instance,
                    connection,
                    database,
                    schema: schema_name,
                },
            });
            out.dirty = true;
        }
        SqlTabMessage::FocusConnectionTab {
            instance,
            connection,
            connection_id,
            database,
            schema,
            default_database,
        } => {
            // Focus an existing tab for this connection if one exists, else
            // open a new one. Only a freshly created tab needs its completion
            // catalog seeded. Clear a leftover picker on the now-active tab so
            // it can't block that editor's keys / context clicks.
            let before = state.active_tab;
            let created = state.focus_or_open_connection_tab(
                instance.clone(),
                connection.clone(),
                connection_id,
                database.clone(),
                schema.clone(),
                default_database.as_deref(),
            );
            let focused_different_tab = state.active_tab != before;
            if focused_different_tab {
                state.close_active_context_picker();
            }
            if created {
                let tab_id = state.tabs.last().map(|t| t.session.id).unwrap_or_default();
                let schema_name = schema.clone().unwrap_or_else(|| "public".to_string());
                out.effects.push(SqlTabEffect::Editor {
                    tab_id,
                    effect: editor::effect::EditorEffect::LoadCompletionCatalog {
                        instance: instance.clone(),
                        connection: connection.clone(),
                        database,
                        schema: schema_name,
                    },
                });
                // Seed the newly created tab with this connection's persisted
                // SQL history (mirrors the original dbm's per-connection history).
                out.effects.push(SqlTabEffect::History {
                    tab_id,
                    effect: history::effect::HistoryEffect::LoadHistory {
                        instance,
                        connection,
                    },
                });
            }
            // Only repaint when focusing a connection actually changed the
            // visible tab state (created a tab or switched to a different one).
            // Re-activating the already-active connection must not count as a
            // redundant redraw and inflate the waste metric.
            out.dirty = created || focused_different_tab;
        }
        SqlTabMessage::SetActiveConnection {
            instance,
            connection,
        } => {
            // Clear a leftover picker on the now-active tab (only when actually
            // switching connections) so it can't block that tab's editor.
            let key = (instance.clone(), connection.clone());
            let switched = state.active_connection.as_ref() != Some(&key);
            state.activate_connection(instance, connection);
            if switched {
                state.close_active_context_picker();
            }
            out.dirty = true;
        }
        SqlTabMessage::ApplyContext {
            tab_id,
            database,
            schema,
        } => {
            if let Some(idx) = state.index_of(tab_id) {
                let tab = &mut state.tabs[idx];
                let changed = tab.session.database.as_ref() != Some(&database)
                    || tab.session.schema.as_ref() != Some(&schema);
                tab.session.database = Some(database);
                tab.session.schema = Some(schema);
                out.dirty = changed;
            } else {
                warn_tab_missing(tab_id);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sql_workspace::sql_tab::update::update;

    #[test]
    fn focus_connection_tab_is_not_dirty_when_already_focused() {
        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        // Focus the same connection again: nothing visible changes (same tab,
        // already active), so it must not repaint — otherwise every connection
        // activate on an existing tab would inflate the waste metric.
        let (s2, _i, _e, dirty) = update(
            SqlTabMessage::FocusConnectionTab {
                instance: "inst".into(),
                connection: "c1".into(),
                connection_id: "id1".into(),
                database: None,
                schema: None,
                default_database: Some("postgres".into()),
            },
            s,
        );
        assert!(!dirty, "re-focusing the active tab must not repaint");
        let _ = s2;
    }
    #[test]
    fn focus_connection_tab_is_dirty_when_opening_a_new_tab() {
        let s = SqlTabState::default();
        let (s2, _i, _e, dirty) = update(
            SqlTabMessage::FocusConnectionTab {
                instance: "inst".into(),
                connection: "c1".into(),
                connection_id: "id1".into(),
                database: None,
                schema: None,
                default_database: Some("postgres".into()),
            },
            s,
        );
        assert!(dirty, "opening a new tab must repaint");
        assert_eq!(s2.tabs.len(), 1);
    }
    #[test]
    fn switching_tab_closes_a_leftover_picker() {
        use crate::features::sql_workspace::sql_tab::editor::context_picker::state::{
            ContextPickerState, PickerColumn,
        };
        // Two tabs on the same connection; tab 0's picker is left open.
        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        s.open_connection_tab("inst".into(), "c1".into(), "id2".into(), None, None, None);
        s.tabs[0].editor.context_picker = ContextPickerState::open(
            PickerColumn::Database,
            "inst".into(),
            "c1".into(),
            "db".into(),
            "public".into(),
        );
        assert!(s.tabs[0].editor.context_picker.open);
        assert_eq!(s.active_tab, Some(1));

        // Switching back to tab 0 closes its leftover picker.
        let (s, _i, _e, _d) = update(SqlTabMessage::Tab(0), s);
        assert_eq!(s.active_tab, Some(0));
        assert!(
            !s.tabs[0].editor.context_picker.open,
            "switching tabs closes a leftover picker"
        );
    }
    #[test]
    fn switching_connection_closes_a_leftover_picker() {
        use crate::features::sql_workspace::sql_tab::editor::context_picker::state::{
            ContextPickerState, PickerColumn,
        };
        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        s.open_connection_tab("inst".into(), "c2".into(), "id2".into(), None, None, None);
        s.tabs[0].editor.context_picker = ContextPickerState::open(
            PickerColumn::Database,
            "inst".into(),
            "c1".into(),
            "db".into(),
            "public".into(),
        );
        assert!(s.tabs[0].editor.context_picker.open);
        assert_eq!(s.active_tab, Some(1));

        // Switching the active connection back to c1 closes the leftover picker.
        let (s, _i, _e, _d) = update(
            SqlTabMessage::SetActiveConnection {
                instance: "inst".into(),
                connection: "c1".into(),
            },
            s,
        );
        assert!(
            !s.tabs[0].editor.context_picker.open,
            "switching connections closes a leftover picker"
        );
    }
    #[test]
    fn focusing_history_clamps_an_oversized_list_to_the_detail_boundary() {
        // A list width that was legal while History was unfocused (up to
        // `history_max`) becomes illegal the moment the detail appears: the
        // zone is `list + detail + splitter`, so entering History must re-clamp
        // the list to `history_max - detail`, or the rendered list would be
        // squeezed below its stored width (a source of redundant repaints).
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;
        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        // Detail not yet visible: list was dragged up to history_max = 93.
        s.tabs[0].focus = SqlFocus::Editor;
        s.tabs[0].splitter.history_pane_width = 93;
        s.tabs[0].splitter.history_max = 93; // 114-wide body: 114 - 20 - 1
        s.tabs[0].history.splitter.detail_pane_width = 40;
        s.history_store.record_success("inst", "c1", "SELECT 1");

        // Enter History -> the detail pane shows, so the list must give way.
        let (s, _i, _e, d) = update(SqlTabMessage::Focus(SqlFocus::History), s);
        assert_eq!(
            s.tabs[0].splitter.history_pane_width, 51,
            "entering History must clamp the list to history_max - detail - border = 93 - 40 - 2"
        );
        assert!(
            d,
            "focusing History with an oversized list must mark the state dirty once"
        );

        // A second, no-op focus (list already within bounds) must not dirty.
        let (s2, _i, _e, d2) = update(SqlTabMessage::Focus(SqlFocus::History), s);
        assert_eq!(s2.tabs[0].splitter.history_pane_width, 51);
        assert!(
            !d2,
            "re-entering History when the list is already legal must not dirty"
        );
    }

    #[test]
    fn focus_away_from_results_unfocuses_clean_detail_editor() {
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;
        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        // Focus the results list, then focus the detail cell editor (clean).
        let (mut s, _i, _e, _d) = update(SqlTabMessage::Focus(SqlFocus::Results), s);
        s.tabs[0].results.detail.focus_editor("abc");
        assert!(s.tabs[0].results.detail.focused);
        assert!(!s.tabs[0].results.detail.dirty);

        let (s2, _i, _e, dirty) = update(SqlTabMessage::Focus(SqlFocus::Editor), s);
        assert!(dirty, "the focus change must repaint");
        assert_eq!(s2.tabs[0].focus, SqlFocus::Editor);
        assert!(
            !s2.tabs[0].results.detail.focused,
            "leaving Results must drop a clean detail editor focus"
        );
    }

    #[test]
    fn focus_away_from_results_blocked_while_detail_draft_dirty() {
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;
        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let (mut s, _i, _e, _d) = update(SqlTabMessage::Focus(SqlFocus::Results), s);
        s.tabs[0].results.detail_open = true;
        s.tabs[0].results.detail.focus_editor("abc");
        s.tabs[0].results.detail.draft = "abx".into();
        s.tabs[0].results.detail.dirty = true;

        let (s2, _i, _e, dirty) = update(SqlTabMessage::Focus(SqlFocus::Editor), s);
        assert!(dirty, "the blocked move still repaints the leave warning");
        assert_eq!(
            s2.tabs[0].focus,
            SqlFocus::Results,
            "an unsaved detail draft must keep focus on Results"
        );
        assert!(s2.tabs[0].results.detail.focused);
        assert!(
            s2.tabs[0].results.detail.leave_warning,
            "the blocked leave must surface the save/discard warning"
        );
    }

    #[test]
    fn focus_away_from_results_blocked_by_dirty_edit_session() {
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;
        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let (mut s, _i, _e, _d) = update(SqlTabMessage::Focus(SqlFocus::Results), s);
        // A dirty edit session on the list (a modified cell).
        s.tabs[0].results.list.edit.enter_edit(&[vec!["1".into()]]);
        s.tabs[0].results.list.edit.apply_cell(0, 0, "x".into());
        assert!(s.tabs[0].results.list.edit.is_dirty());

        let (mut s2, _i, _e, dirty) = update(SqlTabMessage::Focus(SqlFocus::Editor), s);
        assert!(dirty, "the blocked move still repaints the leave warning");
        assert_eq!(
            s2.tabs[0].focus,
            SqlFocus::Results,
            "unsaved list edits must keep focus on Results"
        );
        assert!(
            s2.tabs[0].results.list.leave_warning,
            "the blocked leave must raise the list footer warning"
        );

        // Once clean (rollback) the same move is allowed.
        s2.tabs[0].results.list.edit.rollback();
        assert!(!s2.tabs[0].results.list.edit.is_dirty());
        let (s3, _i, _e, _d) = update(SqlTabMessage::Focus(SqlFocus::Editor), s2);
        assert_eq!(s3.tabs[0].focus, SqlFocus::Editor);
    }
}
