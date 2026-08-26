//! Results list sub-module update.
//!
//! Pure by-value transition over the list state. Query execution runs as a
//! side-channel effect (`ResultsEffect::RunQuery` -> `Services::execute_sql`).

use crate::common::components::search::PaneSearchInput;

use super::msg::ListMessage;
use super::state::ListState;
use super::super::effect::ResultsEffect;

pub fn update(
    msg: ListMessage,
    mut state: ListState,
) -> (ListState, Vec<ResultsEffect>, bool) {
    let mut effects = Vec::new();
    let dirty = match msg {
        ListMessage::SetResult { result, paginated } => {
            state.set_result(result);
            state.query_error = None;
            state.paginated = paginated;
            state.row = 0;
            state.col = 0;
            state.h_scroll = 0;
            state.search.reset();
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
        ListMessage::EditabilityReady { target, blocked } => {
            state.edit_target = target;
            state.edit_blocked_reason = blocked;
            if state.edit_target.is_none() {
                state.exit_edit();
            }
            true
        }
        ListMessage::ClearResult => {
            let changed = state.result.is_some();
            state.result = None;
            state.col_widths.clear();
            state.query_error = None;
            state.row = 0;
            state.col = 0;
            state.edit_target = None;
            state.edit_blocked_reason = None;
            changed
        }
        ListMessage::QueryError { message } => {
            let changed = state.result.is_some()
                || state.query_error.as_deref() != Some(message.as_str());
            state.result = None;
            state.col_widths.clear();
            state.query_error = Some(message);
            state.row = 0;
            state.col = 0;
            state.edit_target = None;
            state.edit_blocked_reason = None;
            changed
        }
        ListMessage::MoveSelection { dr, dc } => {
            let changed = state.move_selection(dr, dc);
            if changed && dc != 0 {
                state.auto_scroll_h();
            }
            changed
        }
        ListMessage::BeginSearch => {
            state.search.reset();
            state.search.start();
            true
        }
        ListMessage::SearchKey(key) => {
            handle_search_key(&mut state, key);
            true
        }
        ListMessage::ResetSelection => {
            let changed = state.row != 0 || state.col != 0 || state.h_scroll != 0
                || state.search.active;
            state.row = 0;
            state.col = 0;
            state.h_scroll = 0;
            state.search.reset();
            changed
        }
        ListMessage::RunQuery {
            instance,
            connection,
            database,
            schema,
            sql,
            paginated,
            page,
            row_limit,
        } => {
            state.query_error = None;
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
        ListMessage::EnterEdit => {
            state.enter_edit();
            true
        }
        ListMessage::ExitEdit => {
            state.exit_edit();
            true
        }
        ListMessage::Rollback => {
            state.rollback_edits();
            true
        }
        ListMessage::AddRow => {
            state.edit_add_row();
            true
        }
        ListMessage::DupRow => {
            state.edit_dup_row();
            true
        }
        ListMessage::DelRow => {
            state.edit_del_row();
            true
        }
        ListMessage::SetRowLimit { limit } => {
            state.row_limit = limit.max(1);
            state.page = 1;
            rerun_query(&state, &mut effects);
            true
        }
        ListMessage::SetPage { page } => {
            let max = super::super::pagination::max_page(
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
        ListMessage::Commit => {
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
        ListMessage::SyncViewport { rows, width } => {
            state.set_viewport(rows, width);
            false
        }
    };
    (state, effects, dirty)
}

/// Re-run the last query with the current page/row-limit.
fn rerun_query(state: &ListState, effects: &mut Vec<ResultsEffect>) {
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

fn handle_search_key(state: &mut ListState, key: crossterm::event::KeyEvent) {
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

    fn sample_result() -> super::super::super::state::QueryResultData {
        super::super::super::state::QueryResultData {
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
        let (mut state, _e, dirty) = update(
            ListMessage::SetResult {
                result: sample_result(),
                paginated: false,
            },
            ListState::default(),
        );
        assert!(state.result.is_some());
        assert!(dirty);

        let (s, _e, dirty) = update(
            ListMessage::QueryError {
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
        let (mut state, _e, _d) = update(
            ListMessage::QueryError {
                message: "boom".into(),
            },
            ListState::default(),
        );
        assert!(state.query_error.is_some());

        let (s, _e, _d) = update(
            ListMessage::SetResult {
                result: sample_result(),
                paginated: false,
            },
            state,
        );
        state = s;
        assert!(state.result.is_some());
        assert!(state.query_error.is_none());
    }

    #[test]
    fn clear_result_clears_query_error_too() {
        let (state, _e, _d) = update(
            ListMessage::QueryError {
                message: "boom".into(),
            },
            ListState::default(),
        );
        assert!(state.query_error.is_some());

        let (s, _e, _d) = update(ListMessage::ClearResult, state);
        assert!(s.query_error.is_none());
    }
}