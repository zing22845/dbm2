//! `sql_tab` feature update.
//!
//! This update is a pure transition: it takes the tab state by value and
//! returns a new one, moving (not cloning) the sub-states it touches. Child
//! messages carry a `tab_id` and are routed to that specific tab, so a child
//! update only ever moves the targeted tab's sub-state out and back — the
//! other tabs are untouched, and no `O(total tabs)` deep clone happens per
//! message. Intents/effects are re-tagged with the same `tab_id` so the
//! cascade lands back on the originating tab.

use super::msg::SqlTabMessage;
use super::state::{SqlFocus, SqlTabState};
use super::intent::SqlTabIntent;
use super::effect::SqlTabEffect;
use super::editor;
use super::history;
use super::results;

/// Update the `sql_tab` state, delegating to child modules.
///
/// The returned `bool` is `dirty`: whether any rendered tab state changed.
/// Messages routed to a missing tab (logged and dropped) report `false`.
pub fn update(
    msg: SqlTabMessage,
    mut state: SqlTabState,
) -> (SqlTabState, Vec<SqlTabIntent>, Vec<SqlTabEffect>, bool) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    let mut dirty = false;
    match msg {
        SqlTabMessage::Tab(visible_idx) => {
            // Map the visible-tab offset to a global index within the active
            // connection's tabs, mirroring the original dbm's `switch_tab_index`.
            let before = state.active_tab;
            if let Some(global) = state.visible_to_global(visible_idx) {
                state.active_tab = global;
                // Clearing a leftover picker on the newly active tab prevents it
                // from blocking that tab's editor (keys / context clicks).
                if global != before {
                    state.close_active_context_picker();
                }
                // Keep the connection's "current tab" bookmark in sync with the
                // active tab, so a new tab opened later (`Alt+t`/`n`) inherits
                // the currently active tab's context rather than a stale one.
                state.remember_active_tab_for_connection();
            }
            dirty = before != state.active_tab;
        }
        SqlTabMessage::Focus(focus) => {
            if let Some(tab) = state.tabs.get_mut(state.active_tab) {
                let changed = tab.focus != focus;
                // Track the sub-pane that was active before entering Results, so
                // Ctrl+Up from Results returns to the previous editor/history
                // pane (mirroring the original dbm's `workspace_upper_pane`).
                if focus == SqlFocus::Results
                    && matches!(tab.focus, SqlFocus::Editor | SqlFocus::History)
                {
                    tab.upper_pane = tab.focus;
                }
                tab.focus = focus;
                dirty = changed;
            }
        }
        SqlTabMessage::OpenTab => {
            state.open_tab();
            dirty = true;
        }
        SqlTabMessage::CloseTab(visible_idx) => {
            // Map the visible offset to a global index before closing.
            if let Some(global) = state.visible_to_global(visible_idx) {
                state.close_tab(global);
                dirty = true;
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
            let tab_id = state
                .tabs
                .last()
                .map(|t| t.session.id)
                .unwrap_or_default();
            let schema_name = schema
                .clone()
                .unwrap_or_else(|| "public".to_string());
            effects.push(SqlTabEffect::Editor {
                tab_id,
                effect: editor::effect::EditorEffect::LoadCompletionCatalog {
                    instance,
                    connection,
                    database,
                    schema: schema_name,
                },
            });
            dirty = true;
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
            if state.active_tab != before {
                state.close_active_context_picker();
            }
            if created {
                let tab_id = state
                    .tabs
                    .last()
                    .map(|t| t.session.id)
                    .unwrap_or_default();
                let schema_name = schema.clone().unwrap_or_else(|| "public".to_string());
                effects.push(SqlTabEffect::Editor {
                    tab_id,
                    effect: editor::effect::EditorEffect::LoadCompletionCatalog {
                        instance,
                        connection,
                        database,
                        schema: schema_name,
                    },
                });
            }
            dirty = true;
        }
        SqlTabMessage::SetActiveConnection { instance, connection } => {
            // Clear a leftover picker on the now-active tab (only when actually
            // switching connections) so it can't block that tab's editor.
            let key = (instance.clone(), connection.clone());
            let switched = state.active_connection.as_ref() != Some(&key);
            state.activate_connection(instance, connection);
            if switched {
                state.close_active_context_picker();
            }
            dirty = true;
        }
        SqlTabMessage::ApplyContext { tab_id, database, schema } => {
            if let Some(idx) = state.index_of(tab_id) {
                let tab = &mut state.tabs[idx];
                let changed = tab.session.database.as_ref() != Some(&database)
                    || tab.session.schema.as_ref() != Some(&schema);
                tab.session.database = Some(database);
                tab.session.schema = Some(schema);
                dirty = changed;
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::SetSplitRatio { tab_id, ratio } => {
            if let Some(idx) = state.index_of(tab_id) {
                state.tabs[idx].set_split_ratio(ratio);
                dirty = true;
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::SetHistoryWidth { tab_id, width } => {
            if let Some(idx) = state.index_of(tab_id) {
                let changed = state.tabs[idx].history_pane_width != width;
                state.tabs[idx].set_history_pane_width(width);
                dirty = changed;
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::NudgeHistoryWidth { tab_id, nudge } => {
            use crate::common::view::splitter::{
                WIDTH_NUDGE_STEP, width_delta_for_right_pane,
            };
            if let Some(idx) = state.index_of(tab_id) {
                let delta = width_delta_for_right_pane(nudge, WIDTH_NUDGE_STEP);
                let current = i32::from(state.tabs[idx].history_pane_width);
                let next = (current + i32::from(delta)).max(0) as u16;
                let changed = state.tabs[idx].history_pane_width != next;
                state.tabs[idx].set_history_pane_width(next);
                dirty = changed;
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::RecallHistory { tab_id, sql } => {
            if let Some(idx) = state.index_of(tab_id) {
                let editor_state = std::mem::take(&mut state.tabs[idx].editor);
                let (s, _i, _e, d) = editor::update::update(
                    editor::msg::EditorMessage::SetSql { sql },
                    editor_state,
                );
                state.tabs[idx].editor = s;
                dirty = d;
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::RunQueryFromEditor { tab_id, sql } => {
            if let Some(idx) = state.index_of(tab_id) {
                let session = &state.tabs[idx].session;
                let instance = session.instance.clone().unwrap_or_default();
                let connection = session
                    .connection
                    .clone()
                    .or_else(|| session.connection_id.clone())
                    .unwrap_or_default();
                let database = session.database.clone();
                let schema = session
                    .schema
                    .clone()
                    .unwrap_or_else(|| "public".to_string());
                let page = state.tabs[idx].results.page.max(1);
                let row_limit = state.tabs[idx].results.row_limit;
                let results_state = std::mem::take(&mut state.tabs[idx].results);
                let (s, i, e, d) = results::update::update(
                    results::msg::ResultsMessage::RunQuery {
                        instance,
                        connection,
                        database,
                        schema,
                        sql,
                        paginated: true,
                        page,
                        row_limit,
                    },
                    results_state,
                );
                state.tabs[idx].results = s;
                dirty = d;
                intents.extend(
                    i.into_iter()
                        .map(|intent| SqlTabIntent::Results { tab_id, intent }),
                );
                effects.extend(
                    e.into_iter()
                        .map(|effect| SqlTabEffect::Results { tab_id, effect }),
                );
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::ToggleTableCompletion { tab_id } => {
            // Ctrl+T toggles table-name completion (TblCmp), only in INSERT mode,
            // matching the original dbm. The header shows the status there.
            if let Some(idx) = state.index_of(tab_id)
                && matches!(state.tabs[idx].editor.editor.mode, edtui::EditorMode::Insert)
            {
                state.tabs[idx].complete_table_names = !state.tabs[idx].complete_table_names;
                dirty = true;
            }
        }
        SqlTabMessage::Editor { tab_id, msg } => {
            let editor::msg::EditorMsg::Message(inner) = msg;
            if let Some(idx) = state.index_of(tab_id) {
                let editor_state = std::mem::take(&mut state.tabs[idx].editor);
                let (s, i, e, d) = editor::update::update(inner, editor_state);
                state.tabs[idx].editor = s;
                dirty = d;
                intents.extend(
                    i.into_iter()
                        .map(|intent| SqlTabIntent::Editor { tab_id, intent }),
                );
                effects.extend(
                    e.into_iter()
                        .map(|effect| SqlTabEffect::Editor { tab_id, effect }),
                );
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::Results { tab_id, msg } => {
            let results::msg::ResultsMsg::Message(inner) = msg;
            if let Some(idx) = state.index_of(tab_id) {
                let results_state = std::mem::take(&mut state.tabs[idx].results);
                let (s, i, e, d) = results::update::update(inner, results_state);
                state.tabs[idx].results = s;
                dirty = d;
                intents.extend(
                    i.into_iter()
                        .map(|intent| SqlTabIntent::Results { tab_id, intent }),
                );
                effects.extend(
                    e.into_iter()
                        .map(|effect| SqlTabEffect::Results { tab_id, effect }),
                );
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::History { tab_id, msg } => {
            let history::msg::HistoryMsg::Message(inner) = msg;
            if let Some(idx) = state.index_of(tab_id) {
                let (instance, connection) = session_key(&state.tabs[idx].session);
                let history_state = std::mem::take(&mut state.tabs[idx].history);
                let selected_sql = history_state.selected_entry(&instance, &connection);
                let (s, i, e, d) = history::update::update(
                    inner,
                    history_state,
                    &instance,
                    &connection,
                    selected_sql,
                    super::history::detail::detail_text_width(40),
                    8,
                );
                state.tabs[idx].history = s;
                dirty = d;
                intents.extend(
                    i.into_iter()
                        .map(|intent| SqlTabIntent::History { tab_id, intent }),
                );
                effects.extend(
                    e.into_iter()
                        .map(|effect| SqlTabEffect::History { tab_id, effect }),
                );
            } else {
                warn_tab_missing(tab_id);
            }
        }
    }
    (state, intents, effects, dirty)
}

/// Derive the `(instance, connection)` key from a tab's session, falling back
/// to the numeric `connection_id` when the display names are not yet bound.
fn session_key(session: &super::session::TabSession) -> (String, String) {
    let instance = session.instance.clone().unwrap_or_default();
    let connection = session
        .connection
        .clone()
        .or_else(|| session.connection_id.clone())
        .unwrap_or_default();
    (instance, connection)
}

/// Log when a routed child message targets a `tab_id` that no longer exists
/// (e.g. its tab was closed). The message is dropped; this is expected for
/// late async results, but worth surfacing so a stale tab id isn't silently
/// swallowed forever.
fn warn_tab_missing(tab_id: usize) {
    tracing::warn!("sql_tab: message targeted a missing tab (tab_id = {tab_id}); dropped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sql_workspace::sql_tab::state::SqlTabState;

    #[test]
    fn toggle_table_completion_only_in_insert_mode() {
        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        s.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        assert!(!s.tabs[0].complete_table_names);

        // In INSERT mode, Ctrl+T toggles TblCmp on.
        let (s, _i, _e, dirty) = update(SqlTabMessage::ToggleTableCompletion { tab_id }, s);
        assert!(dirty);
        assert!(s.tabs[0].complete_table_names, "INSERT mode toggles TblCmp on");
        // Toggling again turns it off.
        let (s, _i, _e, dirty) = update(SqlTabMessage::ToggleTableCompletion { tab_id }, s);
        assert!(dirty);
        assert!(!s.tabs[0].complete_table_names);

        // In NORMAL mode, Ctrl+T is a no-op (matches the original dbm).
        let mut s = s;
        s.tabs[0].editor.editor.mode = edtui::EditorMode::Normal;
        let (s, _i, _e, dirty) = update(SqlTabMessage::ToggleTableCompletion { tab_id }, s);
        assert!(!dirty, "normal mode does not toggle TblCmp");
        assert!(!s.tabs[0].complete_table_names);
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
        assert_eq!(s.active_tab, 1);

        // Switching back to tab 0 closes its leftover picker.
        let (s, _i, _e, _d) = update(SqlTabMessage::Tab(0), s);
        assert_eq!(s.active_tab, 0);
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
        assert_eq!(s.active_tab, 1);

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
}
