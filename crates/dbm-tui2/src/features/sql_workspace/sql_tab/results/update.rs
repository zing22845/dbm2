//! Results feature update.
//!
//! Pure by-value transition over the result set, cell selection, `/` search
//! and detail sub-pane. Query execution runs as a side-channel effect
//! (`ResultsEffect::RunQuery` -> `Services::execute_sql`), emitted here on
//! `RunQuery` and on pagination changes.

use crate::common::components::search::PaneSearchInput;

use super::msg::ResultsMessage;
use super::state::ResultsState;
use super::intent::ResultsIntent;
use super::effect::ResultsEffect;
use super::detail;

pub fn update(
    msg: ResultsMessage,
    mut state: ResultsState,
) -> (ResultsState, Vec<ResultsIntent>, Vec<ResultsEffect>, bool) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    let dirty = match msg {
        ResultsMessage::SetResult { result, paginated } => {
            state.result = Some(result);
            state.query_error = None;
            state.paginated = paginated;
            state.row = 0;
            state.col = 0;
            state.h_scroll = 0;
            state.search.reset();
            state.detail.scroll = 0;
            // Reset the previous editability and re-resolve it against the new
            // result's columns (the query text and connection context were
            // stored by the preceding `RunQuery`).
            state.edit_target = None;
            state.edit_blocked_reason = None;
            let result_columns: Vec<String> = state
                .result
                .as_ref()
                .map(|r| r.columns.iter().map(|c| c.name.clone()).collect())
                .unwrap_or_default();
            if !result_columns.is_empty() && !state.last_sql.is_empty() {
                effects.push(ResultsEffect::CheckEditability {
                    instance: state.last_instance.clone(),
                    connection: state.last_connection.clone(),
                    database: state.last_database.clone(),
                    schema: state.last_schema.clone(),
                    sql: state.last_sql.clone(),
                    result_columns,
                });
            }
            true
        }
        ResultsMessage::EditabilityReady { target, blocked } => {
            state.edit_target = target;
            state.edit_blocked_reason = blocked;
            if state.edit_target.is_none() {
                state.exit_edit();
            }
            true
        }
        ResultsMessage::ClearResult => {
            let changed = state.result.is_some();
            state.result = None;
            state.query_error = None;
            state.row = 0;
            state.col = 0;
            state.detail.scroll = 0;
            state.edit_target = None;
            state.edit_blocked_reason = None;
            changed
        }
        ResultsMessage::QueryError { message } => {
            let changed = state.result.is_some()
                || state.query_error.as_deref() != Some(message.as_str());
            state.result = None;
            state.query_error = Some(message);
            state.row = 0;
            state.col = 0;
            state.detail.scroll = 0;
            state.edit_target = None;
            state.edit_blocked_reason = None;
            changed
        }
        ResultsMessage::MoveSelection { dr, dc } => {
            if state.move_selection(dr, dc) {
                state.detail.scroll = 0;
                true
            } else {
                false
            }
        }
        ResultsMessage::BeginSearch => {
            state.search.reset();
            state.search.start();
            true
        }
        ResultsMessage::SearchKey(key) => {
            handle_search_key(&mut state, key);
            true
        }
        ResultsMessage::ResetSelection => {
            let changed = state.row != 0 || state.col != 0 || state.h_scroll != 0
                || state.search.active || state.detail.scroll != 0;
            state.row = 0;
            state.col = 0;
            state.h_scroll = 0;
            state.search.reset();
            state.detail.scroll = 0;
            changed
        }
        ResultsMessage::RunQuery {
            instance,
            connection,
            database,
            schema,
            sql,
            paginated,
            page,
            row_limit,
        } => {
            state.detail.scroll = 0;
            // A fresh run clears the previous error (the outcome is reported
            // later by `SetResult` / `QueryError`), mirroring the original dbm.
            state.query_error = None;
            // Remember the connection context and query text so the eventual
            // result can resolve editability and so `Commit` can target the
            // same connection.
            state.last_sql = sql.clone();
            state.last_instance = instance.clone();
            state.last_connection = connection.clone();
            state.last_database = database.clone();
            state.last_schema = schema.clone();
            effects.push(ResultsEffect::RunQuery {
                instance,
                connection,
                database,
                schema,
                sql,
                paginated,
                page,
                row_limit,
            });
            true
        }
        ResultsMessage::EnterEdit => {
            state.enter_edit();
            // Load the selected cell into the detail draft baseline.
            if let Some(value) = state.selected_cell() {
                state.detail_baseline = value.clone();
                state.detail_draft = value;
                state.detail_dirty = false;
            }
            true
        }
        ResultsMessage::ExitEdit => {
            state.exit_edit();
            state.detail_baseline.clear();
            state.detail_draft.clear();
            state.detail_dirty = false;
            state.detail_leave_warning = false;
            true
        }
        ResultsMessage::Rollback => {
            state.rollback_edits();
            // Reload the selected cell as the baseline.
            if let Some(value) = state.selected_cell() {
                state.detail_baseline = value.clone();
                state.detail_draft = value;
                state.detail_dirty = false;
            }
            true
        }
        ResultsMessage::AddRow => {
            state.edit_add_row();
            true
        }
        ResultsMessage::DupRow => {
            state.edit_dup_row();
            true
        }
        ResultsMessage::DelRow => {
            state.edit_del_row();
            true
        }
        ResultsMessage::SetDetailDraft { text } => {
            state.detail_draft = text.clone();
            state.detail_dirty =
                super::detail_edit::detail_draft_dirty(&text, &state.detail_baseline);
            state.apply_cell_value(state.row, state.col, text);
            true
        }
        ResultsMessage::SetRowLimit { limit } => {
            state.row_limit = limit.max(1);
            state.page = 1;
            rerun_query(&state, &mut effects);
            true
        }
        ResultsMessage::SetPage { page } => {
            // Clamp to the available pages when known.
            let max = super::pagination::max_page(
                state.result.as_ref().and_then(|r| r.total_rows),
                state.row_limit,
            );
            state.page = page.max(1);
            if let Some(max) = max {
                state.page = state.page.min(max.max(1));
            }
            rerun_query(&state, &mut effects);
            true
        }
        ResultsMessage::Commit => {
            if let Ok(statements) = state.build_commit_statements() {
                effects.push(ResultsEffect::Commit {
                    instance: state.last_instance.clone(),
                    connection: state.last_connection.clone(),
                    database: state.last_database.clone(),
                    schema: state.last_schema.clone(),
                    statements,
                });
            }
            false
        }
        ResultsMessage::Detail(m) => {
            let detail::msg::DetailMsg::Message(inner) = m;
            let detail_state = std::mem::take(&mut state.detail);
            let (s, i, e, d) = detail::update::update(inner, detail_state);
            state.detail = s;
            intents.extend(i.into_iter().map(ResultsIntent::Detail));
            effects.extend(e.into_iter().map(ResultsEffect::Detail));
            d
        }
    };
    (state, intents, effects, dirty)
}

/// Re-run the last query with the current page/row-limit (used after pagination
/// changes). Emits a `RunQuery` effect with the stored session context.
fn rerun_query(state: &ResultsState, effects: &mut Vec<ResultsEffect>) {
    if state.last_sql.is_empty()
        || state.last_instance.is_empty()
        || state.last_connection.is_empty()
    {
        return;
    }
    effects.push(ResultsEffect::RunQuery {
        instance: state.last_instance.clone(),
        connection: state.last_connection.clone(),
        database: state.last_database.clone(),
        schema: state.last_schema.clone(),
        sql: state.last_sql.clone(),
        paginated: state.paginated,
        page: state.page,
        row_limit: state.row_limit,
    });
}

fn handle_search_key(state: &mut ResultsState, key: crossterm::event::KeyEvent) {
    let caps_lock = false;
    let action = match key.code {
        crossterm::event::KeyCode::Esc => {
            state.search.reset();
            PaneSearchInput::Cancelled
        }
        crossterm::event::KeyCode::Enter => {
            state.search.end();
            PaneSearchInput::Applied
        }
        _ => state.search.handle_key(&key, caps_lock),
    };

    if let PaneSearchInput::Navigate { forward } = action {
        let _ = state.move_selection(if forward { 1 } else { -1 }, 0);
        return;
    }
    if matches!(action, PaneSearchInput::QueryChanged | PaneSearchInput::OptionsChanged) {
        state.row = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;

    fn sample_result() -> super::super::state::QueryResultData {
        super::super::state::QueryResultData {
            columns: vec![ColumnInfo {
                name: "id".into(),
                type_name: "int4".into(),
                type_display: "int4".into(),
                comment: None,
            }],
            rows: vec![vec!["1".into()]],
            rows_affected: None,
            total_rows: Some(1),
        }
    }

    #[test]
    fn query_error_clears_result_and_stores_message() {
        let (mut state, _i, _e, dirty) = update(
            ResultsMessage::SetResult {
                result: sample_result(),
                paginated: false,
            },
            ResultsState::default(),
        );
        assert!(state.result.is_some());
        assert!(dirty);

        let (s, _i, _e, dirty) = update(
            ResultsMessage::QueryError {
                message: "relation \"nope\" does not exist".into(),
            },
            state,
        );
        state = s;
        assert!(state.result.is_none());
        assert_eq!(
            state.query_error.as_deref(),
            Some("relation \"nope\" does not exist")
        );
        assert!(dirty);
    }

    #[test]
    fn set_result_clears_previous_query_error() {
        let (mut state, _i, _e, _d) = update(
            ResultsMessage::QueryError {
                message: "boom".into(),
            },
            ResultsState::default(),
        );
        assert!(state.query_error.is_some());

        let (s, _i, _e, _d) = update(
            ResultsMessage::SetResult {
                result: sample_result(),
                paginated: false,
            },
            state,
        );
        state = s;
        assert!(state.result.is_some());
        assert!(state.query_error.is_none(), "success clears the error");
    }

    #[test]
    fn clear_result_clears_query_error_too() {
        let (mut state, _i, _e, _d) = update(
            ResultsMessage::QueryError {
                message: "boom".into(),
            },
            ResultsState::default(),
        );
        assert!(state.query_error.is_some());

        let (s, _i, _e, _d) = update(ResultsMessage::ClearResult, state);
        state = s;
        assert!(state.query_error.is_none());
    }
}
