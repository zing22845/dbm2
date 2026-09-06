//! Messages forwarded to a tab's child panes (editor, results, history).

use super::super::editor;
use super::super::effect::SqlTabEffect;
use super::super::history;
use super::super::intent::SqlTabIntent;
use super::super::msg::SqlTabMessage;
use super::super::results;
use super::super::state::SqlTabState;
use super::{SqlTabOut, session_key, warn_tab_missing};

pub(super) fn apply(msg: SqlTabMessage, state: &mut SqlTabState, out: &mut SqlTabOut) {
    match msg {
        SqlTabMessage::Editor { tab_id, msg } => {
            let editor::msg::EditorMsg::Message(inner) = msg;
            if let Some(idx) = state.index_of(tab_id) {
                let editor_state = std::mem::take(&mut state.tabs[idx].editor);
                let (s, i, e, d) = editor::update::update(inner, editor_state);
                state.tabs[idx].editor = s;
                out.dirty = d;
                out.intents.extend(
                    i.into_iter()
                        .map(|intent| SqlTabIntent::Editor { tab_id, intent }),
                );
                out.effects.extend(
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
                    if !rs.list.last_sql.is_empty() {
                        out.intents.push(SqlTabIntent::History {
                            tab_id,
                            intent: history::intent::HistoryIntent::RecordSuccess {
                                instance: rs.list.last_instance.clone(),
                                connection: rs.list.last_connection.clone(),
                                sql: rs.list.last_sql.clone(),
                            },
                        });
                    }
                    // An editor-initiated run that just succeeded is emptied and
                    // the transient flag dropped (mirroring the original dbm's
                    // `after_sql_run`).
                    if state.tabs[idx].clear_editor_after_run {
                        state.tabs[idx].clear_editor_after_run = false;
                        let editor_state = std::mem::take(&mut state.tabs[idx].editor);
                        let (es, _ei, _ee, ed) = editor::update::update(
                            editor::msg::EditorMessage::ClearAfterRun,
                            editor_state,
                        );
                        state.tabs[idx].editor = es;
                        out.dirty |= ed;
                    }
                } else if matches!(&inner, results::msg::ResultsMessage::QueryError { .. }) {
                    // On failure the buffer is preserved for editing; just drop
                    // the flag so a later pagination re-run cannot wipe it.
                    state.tabs[idx].clear_editor_after_run = false;
                }
                let results_state = std::mem::take(&mut state.tabs[idx].results);
                let (s, i, e, d) = results::update::update(inner, results_state);
                state.tabs[idx].results = s;
                out.dirty |= d;
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
        SqlTabMessage::History { tab_id, msg } => {
            let history::msg::HistoryMsg::Message(inner) = msg;
            if let Some(idx) = state.index_of(tab_id) {
                let (instance, connection) = session_key(&state.tabs[idx].session);

                // Intercept RecordSuccess at parent level: update the shared
                // history store (per connection, shared across all tabs) and
                // emit the persist effect.
                if let history::msg::HistoryMessage::RecordSuccess {
                    ref instance,
                    ref connection,
                    ref sql,
                } = inner
                {
                    state
                        .history_store
                        .record_success(instance, connection, sql);
                    out.effects.push(SqlTabEffect::History {
                        tab_id,
                        effect: history::effect::HistoryEffect::PersistSuccess {
                            instance: instance.clone(),
                            connection: connection.clone(),
                            sql: sql.clone(),
                        },
                    });
                    // The current tab's cursor/detail may need updating too.
                    let history_state = std::mem::take(&mut state.tabs[idx].history);
                    let selected_sql = history_state.list.selected_entry(
                        &state.history_store,
                        instance.as_str(),
                        connection.as_str(),
                    );
                    let (s, _i, _e, _d) = history::update::update(
                        history::msg::HistoryMessage::RecordSuccess {
                            instance: instance.clone(),
                            connection: connection.clone(),
                            sql: sql.clone(),
                        },
                        history_state,
                        &state.history_store,
                        instance.as_str(),
                        connection.as_str(),
                        selected_sql,
                        super::history::detail::layout::detail_text_width(40),
                        8,
                    );
                    state.tabs[idx].history = s;
                    out.dirty = true;
                } else {
                    let history_state = std::mem::take(&mut state.tabs[idx].history);
                    let selected_sql = history_state.list.selected_entry(
                        &state.history_store,
                        instance.as_str(),
                        connection.as_str(),
                    );
                    let (s, i, e, d) = history::update::update(
                        inner,
                        history_state,
                        &state.history_store,
                        instance.as_str(),
                        connection.as_str(),
                        selected_sql,
                        super::history::detail::layout::detail_text_width(40),
                        8,
                    );
                    state.tabs[idx].history = s;
                    out.dirty = d;
                    out.intents.extend(
                        i.into_iter()
                            .map(|intent| SqlTabIntent::History { tab_id, intent }),
                    );
                    out.effects.extend(
                        e.into_iter()
                            .map(|effect| SqlTabEffect::History { tab_id, effect }),
                    );
                }
            } else {
                warn_tab_missing(tab_id);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::editor;
    use super::*;
    use crate::features::sql_workspace::sql_tab::update::update;

    #[test]
    fn update_set_offers_columns_without_tblcmp() {
        // Column completion for `update t set ` must work regardless of TblCmp:
        // the original dbm only gates *table-name* completion on TblCmp, not
        // column completion.
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;
        use crate::features::sql_workspace::sql_tab::editor::state::CompletionCatalog;
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

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
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;
        use crate::features::sql_workspace::sql_tab::editor::state::CompletionCatalog;
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

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
    fn set_result_records_sql_history() {
        use super::super::super::results::msg::{ResultsMessage, ResultsMsg};
        use super::super::super::results::state::QueryResultData;
        use super::history::intent::HistoryIntent;

        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        // Simulate the connection context stored by the preceding `RunQuery`.
        s.tabs[0].results.list.last_instance = "inst".into();
        s.tabs[0].results.list.last_connection = "c1".into();
        s.tabs[0].results.list.last_sql = "SELECT 1".into();

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
        use super::super::super::results::msg::{ResultsMessage, ResultsMsg};
        use super::history::intent::HistoryIntent;

        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        s.tabs[0].results.list.last_sql = "SELECT 1".into();

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
            !intents.iter().any(|i| matches!(
                i,
                SqlTabIntent::History {
                    intent: HistoryIntent::RecordSuccess { .. },
                    ..
                }
            )),
            "a failed query must not record history, got: {intents:?}"
        );
    }
}
