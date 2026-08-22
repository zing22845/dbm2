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
            dirty = before != state.active_tab;
        }
        SqlTabMessage::Focus(focus) => {
            if let Some(tab) = state
                .active_tab
                .and_then(|i| state.tabs.get_mut(i))
            {
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
        SqlTabMessage::EnterHistoryRecall { tab_id } => {
            // Mirrors the original dbm's `enter_history_recall_from_sql`:
            // pin the most recent entry so its detail shows, and move focus
            // to the History pane so the recall list is interactive.
            if let Some(idx) = state.index_of(tab_id) {
                let (instance, connection) = session_key(&state.tabs[idx].session);
                state.tabs[idx].history.pin_most_recent(&instance, &connection);
                state.tabs[idx].focus = SqlFocus::History;
                dirty = true;
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
                let q_instance = state.tabs[idx]
                    .session
                    .instance
                    .clone()
                    .unwrap_or_default();
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
                let page = state.tabs[idx].results.page.max(1);
                let row_limit = state.tabs[idx].results.row_limit;
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
                intents.extend(ri.into_iter().map(|i| SqlTabIntent::Results { tab_id, intent: i }));
                effects.extend(
                    re.into_iter()
                        .map(|e| SqlTabEffect::Results { tab_id, effect: e }),
                );
                if created {
                    effects.push(SqlTabEffect::Editor {
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
            dirty = true;
        }
        SqlTabMessage::ToggleTableCompletion { tab_id } => {
            // Alt+Tab toggles table-name completion (TblCmp), only in INSERT
            // mode. The header shows the status there. The flag is mirrored
            // into the editor so the completion engine can gate table names.
            if let Some(idx) = state.index_of(tab_id)
                && matches!(state.tabs[idx].editor.editor.mode, edtui::EditorMode::Insert)
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
                intents.extend(
                    i.into_iter()
                        .map(|intent| SqlTabIntent::Editor { tab_id, intent }),
                );
                effects.extend(
                    e.into_iter()
                        .map(|effect| SqlTabEffect::Editor { tab_id, effect }),
                );
                dirty = true;
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
                effects.push(SqlTabEffect::Editor {
                    tab_id,
                    effect: editor::effect::EditorEffect::LoadCompletionCatalog {
                        instance,
                        connection,
                        database,
                        schema,
                    },
                });
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
                // A successful result landing mirrors the original dbm's
                // "record on success": log the executed statement in the tab's
                // SQL history (deduped, newest first). The connection context
                // and query text were stored by the preceding `RunQuery`.
                if matches!(&inner, results::msg::ResultsMessage::SetResult { .. }) {
                    let rs = &state.tabs[idx].results;
                    if !rs.last_sql.is_empty() {
                        intents.push(SqlTabIntent::History {
                            tab_id,
                            intent: history::intent::HistoryIntent::RecordSuccess {
                                instance: rs.last_instance.clone(),
                                connection: rs.last_connection.clone(),
                                sql: rs.last_sql.clone(),
                            },
                        });
                    }
                }
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

        // In INSERT mode, Alt+Tab toggles TblCmp on.
        let (s, _i, _e, dirty) = update(SqlTabMessage::ToggleTableCompletion { tab_id }, s);
        assert!(dirty);
        assert!(s.tabs[0].complete_table_names, "INSERT mode toggles TblCmp on");
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
    fn tblcmp_on_after_from_offers_table_names() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        use crate::features::sql_workspace::sql_tab::editor::state::CompletionCatalog;

        let char_key = |c: char| KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        s.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        // Seed the table-name catalog so the popup has tables to offer.
        s.tabs[0].editor.completion_catalog = CompletionCatalog {
            tables: vec!["users".into(), "orders".into()],
            columns_by_table: Default::default(),
        };

        // Alt+Tab enables TblCmp.
        let (mut s, _i, _e, _d) =
            update(SqlTabMessage::ToggleTableCompletion { tab_id }, s);
        assert!(s.tabs[0].complete_table_names);
        assert!(s.tabs[0].editor.complete_table_names);

        // Type `select * from ` and confirm the table popup appears.
        for c in "select * from ".chars() {
            let (s2, _i, _e, _d) = update(
                SqlTabMessage::Editor {
                    tab_id,
                    msg: editor::msg::EditorMsg::Message(editor::msg::EditorMessage::KeyEvent {
                        key: char_key(c),
                        tracked_caps_lock: false,
                    }),
                },
                s,
            );
            s = s2;
        }
        let items = &s.tabs[0].editor.sql_completion.items;
        assert!(s.tabs[0].editor.sql_completion.is_open(), "popup should open after `from `");
        assert!(
            items.iter().any(|i| i.label == "users"),
            "table names must be offered with TblCmp on, got: {items:?}"
        );
    }

    #[test]
    fn update_set_offers_columns_without_tblcmp() {
        // Column completion for `update t set ` must work regardless of TblCmp:
        // the original dbm only gates *table-name* completion on TblCmp, not
        // column completion.
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;
        use crate::features::sql_workspace::sql_tab::editor::state::CompletionCatalog;

        let char_key = |c: char| KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        s.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        // TblCmp stays OFF.
        assert!(!s.tabs[0].complete_table_names);
        // Seed the catalog with the target table's columns.
        let mut catalog = CompletionCatalog {
            tables: vec!["tb1".into()],
            columns_by_table: Default::default(),
        };
        catalog.columns_by_table.insert(
            "tb1".into(),
            vec![
                ColumnInfo {
                    name: "id".into(),
                    type_name: "integer".into(),
                    type_display: "integer".into(),
                    comment: None,
                },
                ColumnInfo {
                    name: "name".into(),
                    type_name: "text".into(),
                    type_display: "text".into(),
                    comment: None,
                },
            ],
        );
        s.tabs[0].editor.completion_catalog = catalog;

        // Type `update tb1 set ` and confirm the column popup appears.
        for c in "update tb1 set ".chars() {
            let (s2, _i, _e, _d) = update(
                SqlTabMessage::Editor {
                    tab_id,
                    msg: editor::msg::EditorMsg::Message(editor::msg::EditorMessage::KeyEvent {
                        key: char_key(c),
                        tracked_caps_lock: false,
                    }),
                },
                s,
            );
            s = s2;
        }
        let items = &s.tabs[0].editor.sql_completion.items;
        assert!(
            s.tabs[0].editor.sql_completion.is_open(),
            "popup should open after `set `"
        );
        assert!(
            items.iter().any(|i| i.label == "name"),
            "columns must be offered for `update t set ` even with TblCmp off, got: {items:?}"
        );
    }

    #[test]
    fn where_whitespace_offers_columns_immediately() {
        // `select * from t where ` must pop the column list right away — the
        // cursor sits after a space, so the clause-keyword auto-open fix is
        // required (it used to only pop after deleting and retyping the space).
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;
        use crate::features::sql_workspace::sql_tab::editor::state::CompletionCatalog;

        let char_key = |c: char| KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        s.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        let mut catalog = CompletionCatalog {
            tables: vec!["tb1".into()],
            columns_by_table: Default::default(),
        };
        catalog.columns_by_table.insert(
            "tb1".into(),
            vec![
                ColumnInfo {
                    name: "id".into(),
                    type_name: "integer".into(),
                    type_display: "integer".into(),
                    comment: None,
                },
                ColumnInfo {
                    name: "status".into(),
                    type_name: "text".into(),
                    type_display: "text".into(),
                    comment: None,
                },
            ],
        );
        s.tabs[0].editor.completion_catalog = catalog;

        // Type the full `select * from tb1 where ` in one pass.
        for c in "select * from tb1 where ".chars() {
            let (s2, _i, _e, _d) = update(
                SqlTabMessage::Editor {
                    tab_id,
                    msg: editor::msg::EditorMsg::Message(editor::msg::EditorMessage::KeyEvent {
                        key: char_key(c),
                        tracked_caps_lock: false,
                    }),
                },
                s,
            );
            s = s2;
        }
        let items = &s.tabs[0].editor.sql_completion.items;
        assert!(
            s.tabs[0].editor.sql_completion.is_open(),
            "popup should open right after `where `"
        );
        assert!(
            items.iter().any(|i| i.label == "status"),
            "columns must be offered after `where `, got: {items:?}"
        );
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
    fn set_result_records_sql_history() {
        use super::super::results::msg::{ResultsMessage, ResultsMsg};
        use super::super::results::state::QueryResultData;
        use super::history::intent::HistoryIntent;

        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        // Simulate the connection context stored by the preceding `RunQuery`.
        s.tabs[0].results.last_instance = "inst".into();
        s.tabs[0].results.last_connection = "c1".into();
        s.tabs[0].results.last_sql = "SELECT 1".into();

        let (_s, intents, _e, _dirty) = update(
            SqlTabMessage::Results {
                tab_id,
                msg: ResultsMsg::Message(ResultsMessage::SetResult {
                    result: QueryResultData {
                        columns: vec![],
                        rows: vec![vec!["1".into()]],
                        rows_affected: None,
                        total_rows: None,
                    },
                    paginated: false,
                }),
            },
            s,
        );

        assert!(
            intents.iter().any(|i| matches!(
                i,
                SqlTabIntent::History { tab_id: t, intent: HistoryIntent::RecordSuccess { instance, connection, sql } }
                    if *t == tab_id && instance == "inst" && connection == "c1" && sql == "SELECT 1"
            )),
            "SetResult must emit a RecordSuccess history intent, got: {intents:?}"
        );
    }

    #[test]
    fn query_error_does_not_record_history() {
        use super::super::results::msg::{ResultsMessage, ResultsMsg};
        use super::history::intent::HistoryIntent;

        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        s.tabs[0].results.last_sql = "SELECT 1".into();

        let (_s, intents, _e, _dirty) = update(
            SqlTabMessage::Results {
                tab_id,
                msg: ResultsMsg::Message(ResultsMessage::QueryError {
                    message: "boom".into(),
                }),
            },
            s,
        );

        assert!(
            !intents
                .iter()
                .any(|i| matches!(i, SqlTabIntent::History { intent: HistoryIntent::RecordSuccess { .. }, .. })),
            "a failed query must not record history, got: {intents:?}"
        );
    }

    #[test]
    fn enter_history_recall_pins_and_focuses_history() {
        // Mirrors the original dbm's `ctrl+r` from the SQL editor: entering
        // recall pins the newest entry and moves focus to the History pane.
        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        // Seed history so there is an entry to pin.
        s.tabs[0]
            .history
            .store
            .record_success("inst", "c1", "SELECT 1");

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
    }
}
