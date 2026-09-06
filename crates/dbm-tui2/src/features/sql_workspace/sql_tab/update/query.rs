//! Running queries: recalling a history entry and executing the editor or
//! table query, plus the completion catalog.

use super::super::editor;
use super::super::effect::SqlTabEffect;
use super::super::intent::SqlTabIntent;
use super::super::msg::SqlTabMessage;
use super::super::results;
use super::super::state::{SqlFocus, SqlTabState};
use super::{SqlTabOut, clamp_list_for_history_detail, session_key, warn_tab_missing};

pub(super) fn apply(msg: SqlTabMessage, state: &mut SqlTabState, out: &mut SqlTabOut) {
    match msg {
        SqlTabMessage::RecallHistory { tab_id, sql } => {
            if let Some(idx) = state.index_of(tab_id) {
                let editor_state = std::mem::take(&mut state.tabs[idx].editor);
                let (s, _i, _e, d) = editor::update::update(
                    editor::msg::EditorMessage::SetSql { sql },
                    editor_state,
                );
                state.tabs[idx].editor = s;
                // After recalling a history entry, focus returns to the editor
                // so the user can immediately start editing the recalled SQL.
                state.tabs[idx].focus = SqlFocus::Editor;
                out.dirty = true;
                out.dirty |= d;
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::EnterHistoryRecall { tab_id } => {
            // Mirrors the original dbm's `enter_history_recall_from_sql`:
            // pin the most recent entry so its detail shows, move focus to the
            // History pane and start the `/` search input so the user can type
            // a query immediately (the recall list is interactive as an input).
            if let Some(idx) = state.index_of(tab_id) {
                let (instance, connection) = session_key(&state.tabs[idx].session);
                state.tabs[idx].history.pin_most_recent(
                    &state.history_store,
                    &instance,
                    &connection,
                );
                state.tabs[idx].history.list.search.start();
                state.tabs[idx].focus = SqlFocus::History;
                out.dirty = true;
                out.dirty |=
                    clamp_list_for_history_detail(&mut state.tabs[idx], &state.history_store);
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::RunQueryFromEditor { tab_id, sql } => {
            if let Some(idx) = state.index_of(tab_id) {
                // This run came from the editor: once it succeeds its buffer is
                // emptied (mirroring the original dbm's `after_sql_run`). The
                // flag is reset when the query's result/error lands.
                state.tabs[idx].clear_editor_after_run = true;
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
                let page = state.tabs[idx].results.list.page.max(1);
                let row_limit = state.tabs[idx].results.list.row_limit;
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
                out.dirty = d;
                out.intents.extend(
                    i.into_iter()
                        .map(|intent| SqlTabIntent::Results { tab_id, intent }),
                );
                out.effects.extend(
                    e.into_iter()
                        .map(|effect| SqlTabEffect::Results { tab_id, effect }),
                );
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::RunTableQuery {
            instance,
            connection,
            connection_id,
            database,
            schema,
            table,
            table_schema,
        } => {
            // Mirror the original dbm's double-click-on-table: run a data query
            // in the connection's active tab (focusing/opening it as needed),
            // filling the editor with `SELECT * FROM "schema"."table"`.
            // If a new tab is created, seed its completion catalog.
            let before = state.active_tab;
            let table_schema = table_schema.or_else(|| schema.clone());
            let created = state.focus_or_open_connection_tab(
                instance.clone(),
                connection.clone(),
                connection_id,
                database.clone(),
                table_schema.clone(),
                None,
            );
            if state.active_tab != before {
                state.close_active_context_picker();
            }
            if let Some(idx) = state.active_tab {
                // Pin the tab's query context to the object's schema so the
                // generated SELECT targets the right namespace (original dbm
                // sets `tab.schema = schema` before running the query).
                state.tabs[idx].session.database = database.clone();
                state.tabs[idx].session.schema = table_schema.clone();
                // Build and apply the SQL, then run it immediately.
                let sql = match &table_schema {
                    Some(s) => format!(
                        "SELECT * FROM {}.{}",
                        results::edit_sql::quote_ident(s),
                        results::edit_sql::quote_ident(&table)
                    ),
                    None => format!("SELECT * FROM {}", results::edit_sql::quote_ident(&table)),
                };
                let editor_state = std::mem::take(&mut state.tabs[idx].editor);
                let (es, _ei, _ee, _ed) = editor::update::update(
                    editor::msg::EditorMessage::SetSql { sql: sql.clone() },
                    editor_state,
                );
                state.tabs[idx].editor = es;
                // Move focus to the Results pane (original dbm ends up there).
                let tab = &mut state.tabs[idx];
                if matches!(tab.focus, SqlFocus::Editor | SqlFocus::History) {
                    tab.upper_pane = tab.focus;
                }
                tab.focus = SqlFocus::Results;
                // Dispatch the query through the results module, exactly as
                // `RunQueryFromEditor` would, using the pinned session context.
                // Snapshot the session context as owned values so no reference
                // into `state.tabs[idx]` outlives the mutable take below.
                let q_instance = state.tabs[idx].session.instance.clone().unwrap_or_default();
                let conn = state.tabs[idx]
                    .session
                    .connection
                    .clone()
                    .or_else(|| state.tabs[idx].session.connection_id.clone())
                    .unwrap_or_default();
                let db = state.tabs[idx].session.database.clone();
                let sch = state.tabs[idx]
                    .session
                    .schema
                    .clone()
                    .unwrap_or_else(|| "public".to_string());
                let tab_id = state.tabs[idx].session.id;
                let page = state.tabs[idx].results.list.page.max(1);
                let row_limit = state.tabs[idx].results.list.row_limit;
                let results_state = std::mem::take(&mut state.tabs[idx].results);
                let (rs, ri, re, _rd) = results::update::update(
                    results::msg::ResultsMessage::RunQuery {
                        instance: q_instance,
                        connection: conn,
                        database: db,
                        schema: sch,
                        sql,
                        paginated: true,
                        page,
                        row_limit,
                    },
                    results_state,
                );
                state.tabs[idx].results = rs;
                out.intents.extend(
                    ri.into_iter()
                        .map(|i| SqlTabIntent::Results { tab_id, intent: i }),
                );
                out.effects.extend(
                    re.into_iter()
                        .map(|e| SqlTabEffect::Results { tab_id, effect: e }),
                );
                if created {
                    out.effects.push(SqlTabEffect::Editor {
                        tab_id,
                        effect: editor::effect::EditorEffect::LoadCompletionCatalog {
                            instance,
                            connection,
                            database,
                            schema: table_schema.unwrap_or_else(|| "public".to_string()),
                        },
                    });
                }
            }
            out.dirty = true;
        }
        SqlTabMessage::ToggleTableCompletion { tab_id } => {
            // Alt+Tab toggles table-name completion (TblCmp), only in INSERT
            // mode. The header shows the status there. The flag is mirrored
            // into the editor so the completion engine can gate table names.
            if let Some(idx) = state.index_of(tab_id)
                && matches!(
                    state.tabs[idx].editor.editor.mode,
                    edtui::EditorMode::Insert
                )
            {
                let on = !state.tabs[idx].complete_table_names;
                state.tabs[idx].complete_table_names = on;
                state.tabs[idx].editor.complete_table_names = on;
                // Recompute the popup so the new setting takes effect
                // immediately (closing the popup when turning TblCmp off,
                // mirroring the original dbm's `toggle_table_name_completion`).
                let editor_state = std::mem::take(&mut state.tabs[idx].editor);
                let (s, i, e, _d) = editor::update::update(
                    editor::msg::EditorMessage::RefreshCompletion,
                    editor_state,
                );
                state.tabs[idx].editor = s;
                out.intents.extend(
                    i.into_iter()
                        .map(|intent| SqlTabIntent::Editor { tab_id, intent }),
                );
                out.effects.extend(
                    e.into_iter()
                        .map(|effect| SqlTabEffect::Editor { tab_id, effect }),
                );
                out.dirty = true;
            }
        }
        SqlTabMessage::ReloadCompletionCatalog { tab_id } => {
            // The editor hit a table-intent slot with no cached table names, so
            // (re)load the tab's completion catalog from the connection.
            if let Some(idx) = state.index_of(tab_id) {
                let instance = state.tabs[idx].session.instance.clone().unwrap_or_default();
                let connection = state.tabs[idx]
                    .session
                    .connection
                    .clone()
                    .or_else(|| state.tabs[idx].session.connection_id.clone())
                    .unwrap_or_default();
                let database = state.tabs[idx].session.database.clone();
                let schema = state.tabs[idx]
                    .session
                    .schema
                    .clone()
                    .unwrap_or_else(|| "public".to_string());
                out.effects.push(SqlTabEffect::Editor {
                    tab_id,
                    effect: editor::effect::EditorEffect::LoadCompletionCatalog {
                        instance,
                        connection,
                        database,
                        schema,
                    },
                });
                out.dirty = true;
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
    fn toggle_table_completion_only_in_insert_mode() {
        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        s.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        assert!(!s.tabs[0].complete_table_names);

        // In INSERT mode, Alt+Tab toggles TblCmp on.
        let (s, _i, _e, dirty) = update(SqlTabMessage::ToggleTableCompletion { tab_id }, s);
        assert!(dirty);
        assert!(
            s.tabs[0].complete_table_names,
            "INSERT mode toggles TblCmp on"
        );
        assert!(
            s.tabs[0].editor.complete_table_names,
            "flag is mirrored into the editor so completion can gate table names"
        );
        // Toggling again turns it off.
        let (s, _i, _e, dirty) = update(SqlTabMessage::ToggleTableCompletion { tab_id }, s);
        assert!(dirty);
        assert!(!s.tabs[0].complete_table_names);
        assert!(!s.tabs[0].editor.complete_table_names);

        // In NORMAL mode, Alt+Tab is a no-op.
        let mut s = s;
        s.tabs[0].editor.editor.mode = edtui::EditorMode::Normal;
        let (s, _i, _e, dirty) = update(SqlTabMessage::ToggleTableCompletion { tab_id }, s);
        assert!(!dirty, "normal mode does not toggle TblCmp");
        assert!(!s.tabs[0].complete_table_names);
        assert!(!s.tabs[0].editor.complete_table_names);
    }
    #[test]
    fn enter_history_recall_pins_focuses_and_starts_search() {
        // Mirrors the original dbm's `ctrl+r` from the SQL editor: entering
        // recall pins the newest entry, moves focus to the History pane, and
        // starts the `/` search input so the next keystroke types a query.
        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        // Seed history so there is an entry to pin.
        s.history_store.record_success("inst", "c1", "SELECT 1");

        let (s, _i, _e, _d) = update(SqlTabMessage::EnterHistoryRecall { tab_id }, s);

        assert_eq!(
            s.tabs[0].focus,
            crate::features::sql_workspace::sql_tab::state::SqlFocus::History,
            "EnterHistoryRecall must move focus to the History pane"
        );
        assert_eq!(
            s.tabs[0].history.detail.pinned_sql.as_deref(),
            Some("SELECT 1"),
            "EnterHistoryRecall must pin the newest entry's detail"
        );
        assert!(
            s.tabs[0].history.list.search.text_input_active(),
            "EnterHistoryRecall must start the history search input (mirroring ctrl+r recall)"
        );
    }
}
