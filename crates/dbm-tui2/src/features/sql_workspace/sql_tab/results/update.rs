//! Results feature update.
//!
//! Pure by-value transition over the result set, cell selection, `/` search
//! and detail sub-pane. Query execution is a deferred effect (not yet wired).

use crate::common::components::search::PaneSearchInput;

use super::msg::ResultsMessage;
use super::state::ResultsState;
use super::intent::ResultsIntent;
use super::effect::ResultsEffect;
use super::detail;

pub fn update(
    msg: ResultsMessage,
    mut state: ResultsState,
) -> (ResultsState, Vec<ResultsIntent>, Vec<ResultsEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    match msg {
        ResultsMessage::SetResult { result, paginated } => {
            state.result = Some(result);
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
        }
        ResultsMessage::EditabilityReady { target, blocked } => {
            state.edit_target = target;
            state.edit_blocked_reason = blocked;
            if state.edit_target.is_none() {
                state.exit_edit();
            }
        }
        ResultsMessage::ClearResult => {
            state.result = None;
            state.row = 0;
            state.col = 0;
            state.detail.scroll = 0;
            state.edit_target = None;
            state.edit_blocked_reason = None;
        }
        ResultsMessage::MoveSelection { dr, dc } => {
            if state.move_selection(dr, dc) {
                state.detail.scroll = 0;
            }
        }
        ResultsMessage::BeginSearch => {
            state.search.reset();
            state.search.start();
        }
        ResultsMessage::SearchKey(key) => {
            handle_search_key(&mut state, key);
        }
        ResultsMessage::ResetSelection => {
            state.row = 0;
            state.col = 0;
            state.h_scroll = 0;
            state.search.reset();
            state.detail.scroll = 0;
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
        }
        ResultsMessage::EnterEdit => {
            state.enter_edit();
            // Load the selected cell into the detail draft baseline.
            if let Some(value) = state.selected_cell() {
                state.detail_baseline = value.clone();
                state.detail_draft = value;
                state.detail_dirty = false;
            }
        }
        ResultsMessage::ExitEdit => {
            state.exit_edit();
            state.detail_baseline.clear();
            state.detail_draft.clear();
            state.detail_dirty = false;
            state.detail_leave_warning = false;
        }
        ResultsMessage::Rollback => {
            state.rollback_edits();
            // Reload the selected cell as the baseline.
            if let Some(value) = state.selected_cell() {
                state.detail_baseline = value.clone();
                state.detail_draft = value;
                state.detail_dirty = false;
            }
        }
        ResultsMessage::AddRow => state.edit_add_row(),
        ResultsMessage::DupRow => state.edit_dup_row(),
        ResultsMessage::DelRow => state.edit_del_row(),
        ResultsMessage::SetDetailDraft { text } => {
            state.detail_draft = text.clone();
            state.detail_dirty =
                super::detail_edit::detail_draft_dirty(&text, &state.detail_baseline);
            state.apply_cell_value(state.row, state.col, text);
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
        }
        ResultsMessage::Detail(m) => {
            let detail::msg::DetailMsg::Message(inner) = m;
            let detail_state = std::mem::take(&mut state.detail);
            let (s, i, e) = detail::update::update(inner, detail_state);
            state.detail = s;
            intents.extend(i.into_iter().map(ResultsIntent::Detail));
            effects.extend(e.into_iter().map(ResultsEffect::Detail));
        }
    }
    (state, intents, effects)
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
