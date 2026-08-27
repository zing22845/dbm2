//! Results feature update.
//!
//! Routes direct and routed messages to the `list` or `detail` sub-feature
//! and coordinates cross-feature concerns (e.g. loading a cell value into the
//! detail draft when entering edit).

use super::msg::{ResultsMessage};
use super::list::msg::ListMsg;
use super::state::ResultsState;
use super::intent::ResultsIntent;
use super::effect::ResultsEffect;

pub fn update(
    msg: ResultsMessage,
    mut state: ResultsState,
) -> (ResultsState, Vec<ResultsIntent>, Vec<ResultsEffect>, bool) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();

    // Resolve the actual sub-feature message and target.
    match msg {
        ResultsMessage::List(list_msg) => {
            let ListMsg::Message(inner) = list_msg;
            let dirty = route_to_list(inner, &mut state, &mut effects);
            (state, intents, effects, dirty)
        }
        ResultsMessage::Detail(detail_msg) => {
            let super::detail::msg::DetailMsg::Message(inner) = detail_msg;
            let detail_state = std::mem::take(&mut state.detail);
            let (s, i, e, d) =
                super::detail::update::update(inner, detail_state);
            state.detail = s;
            intents.extend(i.into_iter().map(ResultsIntent::Detail));
            effects.extend(e.into_iter().map(ResultsEffect::Detail));
            (state, intents, effects, d)
        }
        ResultsMessage::SetDetailDraft { text } => {
            let draft_msg = super::detail::msg::DetailMessage::SetDraft { text: text.clone() };
            let (ds, _di, _de, _dd) =
                super::detail::update::update(draft_msg, state.detail);
            state.detail = ds;
            state.list.apply_cell_value(state.list.row, state.list.col, text);
            (state, intents, effects, true)
        }
        ResultsMessage::ToggleDetail => {
            state.toggle_detail();
            if !state.detail_open {
                let detail_state = std::mem::take(&mut state.detail);
                let (ds, _di, _de, _dd) = super::detail::update::update(
                    super::detail::msg::DetailMessage::ClearDraft,
                    detail_state,
                );
                state.detail = ds;
            } else if let Some(value) = state.list.selected_cell() {
                let detail_state = std::mem::take(&mut state.detail);
                let (ds, _di, _de, _dd) = super::detail::update::update(
                    super::detail::msg::DetailMessage::LoadCell { value },
                    detail_state,
                );
                state.detail = ds;
            }
            (state, intents, effects, true)
        }
        other => {
            let list_msg = other.into_list_message();
            let dirty = route_to_list(list_msg, &mut state, &mut effects);
            (state, intents, effects, dirty)
        }
    }
}

fn route_to_list(
    msg: super::list::msg::ListMessage,
    state: &mut ResultsState,
    effects: &mut Vec<ResultsEffect>,
) -> bool {
    use super::list::msg::ListMessage;

    let is_rollback = matches!(msg, ListMessage::Rollback);
    let is_enter_edit = matches!(msg, ListMessage::EnterEdit);
    let is_exit_edit = matches!(msg, ListMessage::ExitEdit);
    let is_move = matches!(msg, ListMessage::MoveSelection { .. } | ListMessage::SetSelection { .. });
    let resets_detail_scroll = matches!(
        msg,
        ListMessage::SetResult { .. }
            | ListMessage::ClearResult
            | ListMessage::QueryError { .. }
            | ListMessage::ResetSelection
            | ListMessage::RunQuery { .. }
    );

    let list_state = std::mem::take(&mut state.list);
    let (s, e, d) = super::list::update::update(msg, list_state);
    state.list = s;
    effects.extend(e);

    if is_enter_edit
        && let Some(value) = state.list.selected_cell()
    {
        let detail_state = std::mem::take(&mut state.detail);
        let (ds, _di, _de, _dd) = super::detail::update::update(
            super::detail::msg::DetailMessage::LoadCell { value },
            detail_state,
        );
        state.detail = ds;
    }
    if is_exit_edit {
        let detail_state = std::mem::take(&mut state.detail);
        let (ds, _di, _de, _dd) = super::detail::update::update(
            super::detail::msg::DetailMessage::ClearDraft,
            detail_state,
        );
        state.detail = ds;
    }
    if is_rollback {
        let detail_state = std::mem::take(&mut state.detail);
        let (ds, _di, _de, _dd) = super::detail::update::update(
            super::detail::msg::DetailMessage::ClearDraft,
            detail_state,
        );
        state.detail = ds;
        if let Some(value) = state.list.selected_cell() {
            let detail_state = std::mem::take(&mut state.detail);
            let (ds2, _di2, _de2, _dd2) = super::detail::update::update(
                super::detail::msg::DetailMessage::LoadCell { value },
                detail_state,
            );
            state.detail = ds2;
        }
    }
    if is_move || resets_detail_scroll {
        state.detail.scroll = 0;
    }
    if is_move && state.detail_open
        && let Some(value) = state.list.selected_cell()
    {
        let detail_state = std::mem::take(&mut state.detail);
        let (ds, _di, _de, _dd) = super::detail::update::update(
            super::detail::msg::DetailMessage::LoadCell { value },
            detail_state,
        );
        state.detail = ds;
    }

    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;

    fn sample_result() -> crate::features::sql_workspace::sql_tab::results::state::QueryResultData {
        crate::features::sql_workspace::sql_tab::results::state::QueryResultData {
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

    fn sample_result_multirow() -> crate::features::sql_workspace::sql_tab::results::state::QueryResultData {
        crate::features::sql_workspace::sql_tab::results::state::QueryResultData {
            columns: vec![ColumnInfo {
                name: "id".into(),
                type_name: "int4".into(),
                type_display: "int4".into(),
                comment: None,
            }],
            rows: vec![vec!["1".into()], vec!["2".into()]],
            rows_affected: None,
            total_rows: Some(2),
        }
    }

    #[test]
    fn set_result_via_direct_message() {
        let msg = ResultsMessage::SetResult {
            result: sample_result(),
            paginated: false,
        };
        let (state, _i, _e, dirty) = update(msg, ResultsState::default());
        assert!(state.list.result.is_some());
        assert!(dirty);
    }

    #[test]
    fn query_error_clears_result() {
        let msg = ResultsMessage::QueryError {
            message: "boom".into(),
        };
        let (state, _i, _e, _d) = update(msg, ResultsState::default());
        assert!(state.list.result.is_none());
        assert_eq!(state.list.query_error.as_deref(), Some("boom"));
    }

    #[test]
    fn move_selection_via_direct_message() {
        let msg = ResultsMessage::MoveSelection { dr: 1, dc: 0 };
        let mut state = ResultsState::default();
        state.list.result = Some(sample_result_multirow());
        state.list.row = 0;
        let (s, _i, _e, dirty) = update(msg, state);
        assert_eq!(s.list.row, 1);
        assert!(dirty);
    }

    #[test]
    fn set_detail_draft_updates_both_detail_and_list() {
        let msg = ResultsMessage::SetDetailDraft {
            text: "new_value".into(),
        };
        let mut state = ResultsState::default();
        state.list.result = Some(sample_result());
        state.list.row = 0;
        state.list.col = 0;
        state.list.edit_target = Some(crate::features::sql_workspace::sql_tab::results::edit_sql::EditTarget {
            schema: "public".into(),
            table: "t".into(),
            primary_keys: vec!["id".into()],
            columns: vec!["id".into()],
        });
        state.list.enter_edit();
        let (s, _i, _e, dirty) = update(msg, state);
        assert_eq!(s.list.selected_cell().as_deref(), Some("new_value"));
        assert_eq!(s.detail.draft, "new_value");
        assert!(dirty);
    }
}