//! Results feature update.
//!
//! Routes direct and routed messages to the `list` or `detail` sub-feature
//! and coordinates cross-feature concerns (e.g. loading a cell value into the
//! detail draft when entering edit).

use super::effect::ResultsEffect;
use super::intent::ResultsIntent;
use super::list::msg::ListMsg;
use super::msg::ResultsMessage;
use super::state::ResultsState;

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
            let (s, i, e, d) = super::detail::update::update(inner, detail_state);
            state.detail = s;
            intents.extend(i.into_iter().map(ResultsIntent::Detail));
            effects.extend(e.into_iter().map(ResultsEffect::Detail));
            (state, intents, effects, d)
        }
        ResultsMessage::SetDetailDraft { text } => {
            let draft_msg = super::detail::msg::DetailMessage::SetDraft { text: text.clone() };
            let (ds, _di, _de, _dd) = super::detail::update::update(draft_msg, state.detail);
            state.detail = ds;
            state
                .list
                .apply_cell_value(state.list.row, state.list.col, text);
            (state, intents, effects, true)
        }
        ResultsMessage::ToggleDetail => {
            // Closing the detail while its cell editor holds an unsaved draft
            // must not drop the edit (mirroring the original dbm's
            // `allow_leave_detail_focus`): show the leave warning instead.
            if state.detail_open && state.detail.has_unsaved_draft() {
                state.detail.leave_warning = true;
                return (state, intents, effects, true);
            }
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
        // —— Detail cell editor focus (whole-edit session only) ——
        // Enter (edit session active): open the detail pane if needed and focus
        // the embedded editor on the selected cell, loading its value as the
        // draft baseline.
        ResultsMessage::FocusDetail => {
            if !state.list.edit.editing
                || state.detail.focused
                || state.list.edit.deleted.contains(&state.list.row)
            {
                return (state, intents, effects, false);
            }
            let Some(value) = state.list.selected_cell() else {
                return (state, intents, effects, false);
            };
            if !state.detail_open {
                state.open_detail();
            }
            state.detail.focus_editor(&value);
            (state, intents, effects, true)
        }
        // Esc from a Normal-mode editor: return to table focus. A dirty draft
        // blocks the leave (footer shows the save/discard hint).
        ResultsMessage::UnfocusDetail => {
            if !state.detail.focused {
                return (state, intents, effects, false);
            }
            if state.detail.dirty {
                state.detail.leave_warning = true;
                (state, intents, effects, true)
            } else {
                state.detail.unfocus();
                (state, intents, effects, true)
            }
        }
        // Ctrl+S while the detail editor is focused: write the draft into the
        // edit session (`dirty_cells` + live row, commit count refreshed) and
        // mark the draft clean. The editor stays focused for further edits.
        ResultsMessage::SaveDetailCell => {
            if !state.detail.focused || !state.detail.dirty {
                return (state, intents, effects, false);
            }
            let text = state.detail.current_text();
            let row = state.list.row;
            let col = state.list.col;
            state.list.apply_cell_value(row, col, text.clone());
            state.detail.baseline = text;
            state.detail.sync_draft_from_editor();
            (state, intents, effects, true)
        }
        // Ctrl+U while the detail editor is focused: discard the draft back to
        // the cell's baseline (no edit-session change).
        ResultsMessage::DiscardDetailCell => {
            if !state.detail.focused || !state.detail.dirty {
                return (state, intents, effects, false);
            }
            let baseline = state.detail.baseline.clone();
            if let Some(host) = state.detail.editor.as_mut() {
                crate::common::editor::set_editor_text(&mut host.editor, &baseline);
            }
            state.detail.sync_draft_from_editor();
            (state, intents, effects, true)
        }
        // Post-commit flow: a successful commit exits edit mode (clearing the
        // snapshots / detail draft) and re-runs the last query so the committed
        // rows refresh on screen, mirroring the original dbm's
        // `apply_commit_completion` + `enqueue_refresh`. A failed commit leaves
        // the edit session intact (the round surfaces the error in the footer).
        ResultsMessage::CommitOutcome { ok } => {
            if ok {
                route_to_list(
                    super::list::msg::ListMessage::ExitEdit,
                    &mut state,
                    &mut effects,
                );
                let list = &state.list;
                let run = super::list::msg::ListMessage::RunQuery {
                    instance: list.last_instance.clone(),
                    connection: list.last_connection.clone(),
                    database: list.last_database.clone(),
                    schema: list.last_schema.clone(),
                    sql: list.last_sql.clone(),
                    paginated: list.paginated,
                    page: list.page,
                    row_limit: list.row_limit,
                };
                route_to_list(run, &mut state, &mut effects);
                (state, intents, effects, true)
            } else {
                (state, intents, effects, false)
            }
        }
        // Copy the focused detail cell editor's selection (mouse or v-mode) to
        // the system clipboard. The buffer/mode are untouched, so the selection
        // stays highlighted for further edits.
        ResultsMessage::CopyDetailSelection => {
            let Some(host) = state.detail.editor.as_ref() else {
                return (state, intents, effects, false);
            };
            let Some(selection) = host.editor.selection.as_ref() else {
                return (state, intents, effects, false);
            };
            let text = selection.copy_from(&host.editor.lines).to_string();
            if text.is_empty() {
                return (state, intents, effects, false);
            }
            effects.push(ResultsEffect::CopySelection { text });
            (state, intents, effects, true)
        }
        // A key routed to the focused detail cell editor. It falls through to
        // the detail update (which refuses it when no editor is focused).
        ResultsMessage::DetailEditorKey(key) => {
            let key_msg = super::detail::msg::DetailMessage::KeyEvent {
                key,
                tracked_caps_lock: false,
            };
            let detail_state = std::mem::take(&mut state.detail);
            let (s, i, e, d) = super::detail::update::update(key_msg, detail_state);
            state.detail = s;
            intents.extend(i.into_iter().map(ResultsIntent::Detail));
            effects.extend(e.into_iter().map(ResultsEffect::Detail));
            (state, intents, effects, d)
        }
        other => {
            // While the detail cell editor is focused, a selection move (from a
            // mouse click / wheel) must not clobber the in-progress draft: with
            // a dirty draft it is refused (leave warning), otherwise the editor
            // is unfocused first so the move proceeds on the table.
            let is_move = matches!(
                other,
                ResultsMessage::MoveSelection { .. } | ResultsMessage::SetSelection { .. }
            );
            if is_move && state.detail.focused {
                if state.detail.dirty {
                    state.detail.leave_warning = true;
                    return (state, intents, effects, true);
                }
                state.detail.unfocus();
            }
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
    let is_move = matches!(
        msg,
        ListMessage::MoveSelection { .. } | ListMessage::SetSelection { .. }
    );
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

    if is_enter_edit && let Some(value) = state.list.selected_cell() {
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
    // While the detail cell editor is focused its draft owns the current cell:
    // a table selection move must NOT reload (and clobber) the draft. Only the
    // key layer (editor focus) and mouse (unfocus-first) move the selection.
    if is_move
        && state.detail_open
        && !state.detail.focused
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

    fn sample_result_multirow()
    -> crate::features::sql_workspace::sql_tab::results::state::QueryResultData {
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
    fn commit_outcome_ok_exits_edit_and_reruns_last_query() {
        let mut state = ResultsState::default();
        state.list.result = Some(sample_result_multirow());
        state.list.last_instance = "inst".into();
        state.list.last_connection = "c1".into();
        state.list.last_schema = "public".into();
        state.list.last_sql = "select * from t".into();
        state.list.edit_target = Some(
            crate::features::sql_workspace::sql_tab::results::edit_sql::EditTarget {
                schema: "public".into(),
                table: "t".into(),
                primary_keys: vec!["id".into()],
                columns: vec!["id".into()],
            },
        );
        state.list.enter_edit();
        assert!(state.list.edit.editing);
        let (s, _i, effects, dirty) = update(ResultsMessage::CommitOutcome { ok: true }, state);
        assert!(
            !s.list.edit.editing,
            "a successful commit must exit edit mode"
        );
        assert!(
            effects.iter().any(|e| matches!(
                e,
                ResultsEffect::RunQuery { sql, .. } if sql == "select * from t"
            )),
            "a successful commit must re-run the last query, got: {effects:?}"
        );
        assert!(dirty, "the post-commit repaint must be requested");
    }

    #[test]
    fn commit_outcome_failure_keeps_edit_session() {
        let mut state = ResultsState::default();
        state.list.result = Some(sample_result_multirow());
        state.list.edit_target = Some(
            crate::features::sql_workspace::sql_tab::results::edit_sql::EditTarget {
                schema: "public".into(),
                table: "t".into(),
                primary_keys: vec!["id".into()],
                columns: vec!["id".into()],
            },
        );
        state.list.enter_edit();
        let (s, _i, effects, dirty) = update(ResultsMessage::CommitOutcome { ok: false }, state);
        assert!(
            s.list.edit.editing,
            "a failed commit must keep the edit session"
        );
        assert!(effects.is_empty());
        assert!(!dirty);
    }

    #[test]
    fn del_chord_requires_second_press_within_window() {
        let mut state = ResultsState::default();
        state.list.result = Some(sample_result_multirow());
        state.list.row = 0;
        state.list.edit_target = Some(
            crate::features::sql_workspace::sql_tab::results::edit_sql::EditTarget {
                schema: "public".into(),
                table: "t".into(),
                primary_keys: vec!["id".into()],
                columns: vec!["id".into()],
            },
        );
        state.list.enter_edit();
        // A lone `d` arms the chord and deletes nothing.
        let (s, _i, _e, dirty) = update(ResultsMessage::DelChord, state);
        assert!(!dirty, "a lone d must not delete");
        assert!(s.list.edit.editing);
        // A second `d` within the window deletes the selected row.
        let (s2, _i2, _e2, dirty2) = update(ResultsMessage::DelChord, s);
        assert!(dirty2, "the second d must delete the row");
        assert!(
            s2.list.edit.deleted.contains(&0),
            "the selected loaded row must be marked deleted"
        );
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
        state.list.edit_target = Some(
            crate::features::sql_workspace::sql_tab::results::edit_sql::EditTarget {
                schema: "public".into(),
                table: "t".into(),
                primary_keys: vec!["id".into()],
                columns: vec!["id".into()],
            },
        );
        state.list.enter_edit();
        let (s, _i, _e, dirty) = update(msg, state);
        assert_eq!(s.list.selected_cell().as_deref(), Some("new_value"));
        assert_eq!(s.detail.draft, "new_value");
        assert!(dirty);
    }

    /// A results state with an active edit session and one editable row.
    fn editing_state() -> ResultsState {
        let mut state = ResultsState::default();
        state.list.result = Some(sample_result());
        state.list.row = 0;
        state.list.col = 0;
        state.list.edit_target = Some(
            crate::features::sql_workspace::sql_tab::results::edit_sql::EditTarget {
                schema: "public".into(),
                table: "t".into(),
                primary_keys: vec!["id".into()],
                columns: vec!["id".into()],
            },
        );
        state.list.enter_edit();
        state
    }

    fn press(c: char) -> crossterm::event::KeyEvent {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn focus_detail_requires_edit_session_and_loads_selected_cell() {
        // Edit session off: no-op.
        let state = ResultsState::default();
        let (s, _i, _e, dirty) = update(ResultsMessage::FocusDetail, state);
        assert!(!dirty);
        assert!(!s.detail.focused);

        // Edit session on: opens the detail (if closed) and focuses a draft
        // editor seeded with the selected cell.
        let (s, _i, _e, dirty) = update(ResultsMessage::FocusDetail, editing_state());
        assert!(dirty);
        assert!(s.detail_open, "FocusDetail must open the detail pane");
        assert!(s.detail.focused);
        assert!(
            s.detail.editor.is_some(),
            "a focused detail must hold an editor"
        );
        assert_eq!(s.detail.baseline, "1");
        assert_eq!(s.detail.draft, "1");
    }

    /// Focus the detail editor, place it in Insert at end-of-line, then type
    /// `text`. Returns the resulting state (draft modified, dirty).
    fn focus_and_type(state: ResultsState, text: &str) -> ResultsState {
        let (mut s, _i, _e, _d) = update(ResultsMessage::FocusDetail, state);
        // Enter Insert and move to EOL so typing appends predictably.
        let host = s
            .detail
            .editor
            .as_mut()
            .expect("focused detail holds an editor");
        host.editor.mode = edtui::EditorMode::Insert;
        crate::common::editor::move_cursor_to_eol(&mut host.editor);
        for c in text.chars() {
            let (s2, _i, _e, _d) = update(ResultsMessage::DetailEditorKey(press(c)), s);
            s = s2;
        }
        s
    }

    #[test]
    fn typing_in_focused_editor_drifts_draft_only() {
        // The draft starts as the cell value; type an extra char.
        let s = focus_and_type(editing_state(), "9");
        assert!(s.detail.dirty, "typing must dirty the draft");
        assert_eq!(s.detail.draft, "19");
        assert_eq!(
            s.list.selected_cell().as_deref(),
            Some("1"),
            "typing must NOT touch the table cell (draft + save model)"
        );
    }

    #[test]
    fn save_detail_cell_writes_cell_and_commit_count() {
        let s = focus_and_type(editing_state(), "9");
        assert!(s.detail.dirty);
        assert_eq!(
            s.list.commit_row_count(),
            0,
            "dirty draft alone must not count"
        );

        let (s3, _i, _e, dirty) = update(ResultsMessage::SaveDetailCell, s);
        assert!(dirty);
        assert!(!s3.detail.dirty, "save clears the draft");
        assert_eq!(
            s3.list.selected_cell().as_deref(),
            Some("19"),
            "save applies the draft to the table cell"
        );
        assert_eq!(
            s3.list.commit_row_count(),
            1,
            "saved cell must count into the commit"
        );
        assert!(
            s3.detail.focused,
            "save keeps the editor focused for more edits"
        );
    }

    #[test]
    fn discard_detail_cell_reverts_draft_without_committing() {
        let s = focus_and_type(editing_state(), "9");
        assert!(s.detail.dirty);

        let (s3, _i, _e, dirty) = update(ResultsMessage::DiscardDetailCell, s);
        assert!(dirty);
        assert!(!s3.detail.dirty);
        assert_eq!(s3.detail.draft, "1", "discard reverts to the baseline");
        assert_eq!(s3.list.selected_cell().as_deref(), Some("1"));
        assert_eq!(s3.list.commit_row_count(), 0);
        assert!(s3.detail.focused, "discard keeps the editor focused");
    }

    #[test]
    fn unfocus_detail_blocked_while_dirty_allowed_when_clean() {
        let s = focus_and_type(editing_state(), "9");

        // Dirty: Esc (unfocus) is refused and a leave warning is raised.
        let (s3, _i, _e, dirty) = update(ResultsMessage::UnfocusDetail, s);
        assert!(dirty);
        assert!(
            s3.detail.focused,
            "dirty draft must block leaving the editor"
        );
        assert!(s3.detail.leave_warning);

        // Discard, then leaving is allowed.
        let (s4, _i, _e, _d) = update(ResultsMessage::DiscardDetailCell, s3);
        let (s5, _i, _e, dirty) = update(ResultsMessage::UnfocusDetail, s4);
        assert!(dirty);
        assert!(!s5.detail.focused, "clean draft may leave the editor");
        assert!(
            s5.detail.editor.is_none(),
            "leaving drops the scratch editor"
        );
    }

    #[test]
    fn selection_move_blocked_while_dirty_focused() {
        let s = focus_and_type(editing_state(), "9");
        assert!(s.detail.dirty);

        // A table selection move (mouse / wheel) while the draft is dirty must
        // be refused so the draft is not clobbered by a reload.
        let (s3, _i, _e, dirty) = update(ResultsMessage::MoveSelection { dr: 1, dc: 0 }, s);
        assert!(dirty, "the blocked move still repaints the leave warning");
        assert!(s3.list.row == 0, "the move must not apply");
        assert!(s3.detail.focused);
        assert_eq!(s3.detail.draft, "19");
    }

    #[test]
    fn exit_edit_clears_detail_editor_focus() {
        let s = focus_and_type(editing_state(), "9");
        assert!(s.detail.focused);
        let (s3, _i, _e, dirty) = update(ResultsMessage::ExitEdit, s);
        assert!(dirty);
        assert!(!s3.list.edit.editing);
        assert!(!s3.detail.focused);
        assert!(s3.detail.editor.is_none());
        assert!(!s3.detail.dirty);
    }

    #[test]
    fn copy_detail_selection_emits_clipboard_effect() {
        // Focus the editor and paint a (1-char) selection over the draft.
        let (mut s, _i, _e, _d) = update(ResultsMessage::FocusDetail, editing_state());
        {
            let host = s.detail.editor.as_mut().expect("focused editor");
            host.editor.selection = Some(edtui::Selection::new(
                edtui::Index2::new(0, 0),
                edtui::Index2::new(0, 0),
            ));
        }
        let (s2, _i, effects, dirty) = update(ResultsMessage::CopyDetailSelection, s);
        assert!(dirty);
        assert_eq!(s2.detail.draft, "1", "copy must not touch the draft");
        let copied: Vec<&ResultsEffect> = effects
            .iter()
            .filter(|e| matches!(e, ResultsEffect::CopySelection { .. }))
            .collect();
        assert_eq!(copied.len(), 1, "a selection must copy to the clipboard");
    }

    #[test]
    fn copy_detail_selection_noop_without_editor_or_selection() {
        // No edit session → no focused editor → copy is a no-op.
        let s = ResultsState::default();
        let (_s2, _i, effects, dirty) = update(ResultsMessage::CopyDetailSelection, s);
        assert!(!dirty);
        assert!(effects.is_empty());

        // Focused editor but no selection → no-op too.
        let (s, _i, _e, _d) = update(ResultsMessage::FocusDetail, editing_state());
        let (_s2, _i, effects, dirty) = update(ResultsMessage::CopyDetailSelection, s);
        assert!(!dirty);
        assert!(effects.is_empty());
    }
}
