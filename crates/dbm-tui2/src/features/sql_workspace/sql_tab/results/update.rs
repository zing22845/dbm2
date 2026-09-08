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

/// Whether `key` is an *editing* key for the detail editor: one that would
/// change the buffer text or switch it into Insert mode. Probed on a scratch
/// clone (editor *and* handler) so the live editor is untouched. Decides when a
/// focused detail that has no active edit session must auto-start one before
/// the key is applied.
fn detail_key_would_edit(
    key: &crossterm::event::KeyEvent,
    host: &super::detail::state::DetailEditor,
) -> bool {
    if !crate::common::editor::accepts_key_event(key) {
        return false;
    }
    let mut probe = host.clone();
    let before = crate::common::editor::editor_text(&probe.editor);
    probe.handler.on_key_event(*key, &mut probe.editor);
    crate::common::editor::editor_text(&probe.editor) != before
        || probe.editor.mode == edtui::EditorMode::Insert
}

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
        // —— Detail cell editor focus ——
        // Enter (or a single click on the open detail pane) opens the detail
        // (if needed) and focuses the embedded editor on the selected cell,
        // loading its value as the draft baseline. This works outside a
        // whole-result edit session too (mirroring the original dbm's
        // `open_results_cell_detail`): motion keys / Visual selection / copy
        // keep working in Normal mode, and the first *editing* key auto-starts
        // the edit session (see `DetailEditorKey`).
        ResultsMessage::FocusDetail => {
            if state.detail.focused || state.list.edit.deleted.contains(&state.list.row) {
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
            // Saving resolves any blocked-leave state so the footer hint clears.
            state.detail.leave_warning = false;
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
            // Discarding resolves any blocked-leave state so the footer hint clears.
            state.detail.leave_warning = false;
            (state, intents, effects, true)
        }
        // Esc / focus-leave attempted while the edit session has unsaved
        // changes (dirty cells / deleted rows / pending inserts): block the
        // leave and surface the reason on the results footer instead of
        // silently dropping the edits (mirroring the detail draft's
        // save/discard gate). A clean session never sends this.
        ResultsMessage::EditLeaveAttempt => {
            if state.list.edit.editing && state.list.edit.is_dirty() {
                state.list.leave_warning = true;
                (state, intents, effects, true)
            } else {
                (state, intents, effects, false)
            }
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
        // When the whole-result edit session is not yet active, an *editing*
        // key (one that would change the buffer text or enter Insert) auto-starts
        // it first — mirroring the original dbm's `enter_edit_mode` from inside
        // the detail section. Pure navigation keys (motion, Visual selection,
        // copy) never start a session and keep working in Normal/Visual mode.
        ResultsMessage::DetailEditorKey(key) => {
            if state.detail.focused && !state.list.edit.editing && state.detail.editor.is_some() {
                let host = state.detail.editor.as_ref().expect("editor checked above");
                if detail_key_would_edit(&key, host) {
                    route_to_list(
                        super::list::msg::ListMessage::EnterEdit,
                        &mut state,
                        &mut effects,
                    );
                    if !state.list.edit.editing {
                        // The result is not editable (no edit target / blocked):
                        // ignore the editing key and keep the editor in Normal
                        // mode, so the detail remains a read-only viewer.
                        return (state, intents, effects, true);
                    }
                }
            }
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
    // Rolling back, exiting the session, or replacing the result resolves the
    // blocked-leave warning.
    if is_exit_edit || is_rollback || resets_detail_scroll {
        state.list.leave_warning = false;
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

    /// A results state with one *editable* row, but **no** active edit session
    /// yet (editability resolved, editing off).
    fn editable_state() -> ResultsState {
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
        state
    }

    /// A results state with an active edit session and one editable row.
    fn editing_state() -> ResultsState {
        let mut state = editable_state();
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
    fn focus_detail_requires_a_cell_and_loads_editor_without_edit_session() {
        // No result → nothing to focus (no-op).
        let state = ResultsState::default();
        let (s, _i, _e, dirty) = update(ResultsMessage::FocusDetail, state);
        assert!(!dirty);
        assert!(!s.detail.focused);

        // With a result but NO edit session: Enter still opens + focuses the
        // editor on the selected cell (original dbm's `open_results_cell_detail`).
        let (s, _i, _e, dirty) = update(ResultsMessage::FocusDetail, editable_state());
        assert!(dirty);
        assert!(s.detail_open, "FocusDetail must open the detail pane");
        assert!(s.detail.focused);
        assert!(
            s.detail.editor.is_some(),
            "a focused detail must hold an editor"
        );
        assert!(
            !s.list.edit.editing,
            "focusing alone must NOT start an edit session"
        );
        assert_eq!(s.detail.baseline, "1");
        assert_eq!(s.detail.draft, "1");

        // In an active edit session it behaves the same.
        let (s, _i, _e, dirty) = update(ResultsMessage::FocusDetail, editing_state());
        assert!(dirty);
        assert!(s.detail.focused);
        assert!(s.list.edit.editing);
        assert_eq!(s.detail.draft, "1");
    }

    #[test]
    fn editing_key_in_focused_detail_without_session_auto_starts_edit() {
        // Enter (focus) on an editable result with NO session, then an editing
        // key (`i`) must auto-start the whole-result edit session before the
        // key is applied — mirroring the original dbm's `enter_edit_mode`.
        let (s, _i, _e, _d) = update(ResultsMessage::FocusDetail, editable_state());
        assert!(!s.list.edit.editing);

        let (s2, _i, _e, dirty) = update(ResultsMessage::DetailEditorKey(press('i')), s);
        assert!(dirty);
        assert!(
            s2.list.edit.editing,
            "`i` in a focused detail must auto-start the edit session"
        );
        assert!(
            s2.detail
                .editor
                .as_ref()
                .is_some_and(|h| { h.editor.mode == edtui::EditorMode::Insert }),
            "after `i` the editor must be in Insert mode"
        );
        assert!(!s2.detail.dirty, "`i` alone must not dirty the draft");

        // A deletion key (`x`) auto-starts the session too and edits right away.
        let (s3, _i, _e, _d) = update(ResultsMessage::FocusDetail, editable_state());
        let (s4, _i, _e, dirty) = update(ResultsMessage::DetailEditorKey(press('x')), s3);
        assert!(dirty);
        assert!(s4.list.edit.editing, "`x` must auto-start the edit session");
        assert!(
            s4.detail.dirty,
            "`x` must actually delete the char under the cursor"
        );
    }

    #[test]
    fn nav_key_in_focused_detail_without_session_stays_normal() {
        // Motion keys keep working without ever opening an edit session, so the
        // focused detail is a full vi viewer/copy surface until an editing key.
        let mut sample = sample_result();
        sample.rows = vec![vec!["hello".into()]];
        let mut state = ResultsState::default();
        state.list.result = Some(sample);
        state.list.row = 0;
        state.list.col = 0;
        let (s, _i, _e, _d) = update(ResultsMessage::FocusDetail, state);
        // Park the caret at the start so the `l` motion visibly moves it.
        let mut s = s;
        {
            let host = s.detail.editor.as_mut().expect("focused editor");
            host.editor.cursor = edtui::Index2::new(0, 0);
        }
        let (s2, _i, _e, dirty) = update(ResultsMessage::DetailEditorKey(press('l')), s);
        assert!(dirty, "a motion key repaints the moved caret");
        assert!(
            !s2.list.edit.editing,
            "motion keys must NOT open an edit session"
        );
        assert!(
            s2.detail.editor.as_ref().is_some_and(|h| {
                h.editor.mode == edtui::EditorMode::Normal && h.editor.cursor.col > 0
            }),
            "the caret must have moved within Normal mode"
        );
    }

    #[test]
    fn editing_key_ignored_when_result_not_editable() {
        // A result with no edit target (e.g. a join / aggregate): focusing the
        // detail works for reading, but an editing key is refused and the
        // editor stays in Normal with the buffer untouched.
        let mut state = ResultsState::default();
        state.list.result = Some(sample_result());
        state.list.row = 0;
        state.list.col = 0;
        let (s, _i, _e, _d) = update(ResultsMessage::FocusDetail, state);
        assert!(s.detail.focused);
        assert!(!s.list.edit.editing);

        let (s2, _i, _e, dirty) = update(ResultsMessage::DetailEditorKey(press('i')), s);
        assert!(dirty, "the refused key still repaints (defensive)");
        assert!(!s2.list.edit.editing);
        assert!(
            s2.detail
                .editor
                .as_ref()
                .is_some_and(|h| { h.editor.mode == edtui::EditorMode::Normal }),
            "editor must stay Normal when the result cannot be edited"
        );
        assert_eq!(s2.detail.draft, "1", "buffer must be untouched");
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
    fn esc_leave_attempt_warns_on_dirty_and_rollback_clears_it() {
        // A dirty edit session (a modified cell) blocks the Esc exit.
        let mut state = editable_state();
        state.list.enter_edit();
        state.list.edit.apply_cell(0, 0, "x".into());
        assert!(state.list.edit.is_dirty());

        let (s, _i, _e, dirty) = update(ResultsMessage::EditLeaveAttempt, state);
        assert!(dirty);
        assert!(s.list.leave_warning, "blocked leave must raise the warning");
        assert!(s.list.edit.editing, "the session must survive");

        // Rolling back resolves the edits and clears the warning.
        let (s2, _i, _e, dirty) = update(ResultsMessage::Rollback, s);
        assert!(dirty);
        assert!(!s2.list.edit.is_dirty());
        assert!(
            !s2.list.leave_warning,
            "rollback must clear the blocked-leave warning"
        );
    }

    #[test]
    fn esc_leave_attempt_noop_on_clean_session() {
        // A clean session has nothing to lose; the block must not fire.
        let mut state = editable_state();
        state.list.enter_edit();
        assert!(!state.list.edit.is_dirty());
        let (s, _i, _e, dirty) = update(ResultsMessage::EditLeaveAttempt, state);
        assert!(!dirty);
        assert!(!s.list.leave_warning);
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
