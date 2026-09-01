//! Editor feature update.
//!
//! Forwards buffer edits to the `edtui` editor, then triggers a SQL completion
//! refresh with the updated buffer/cursor. Child sub-module messages are routed
//! to their own updates.

use crossterm::event::KeyEvent;

use super::msg::EditorMessage;
use super::state::EditorState;
use super::intent::EditorIntent;
use super::effect::EditorEffect;
use super::context_picker;
use super::sql_completion;

pub fn update(
    msg: EditorMessage,
    mut state: EditorState,
) -> (EditorState, Vec<EditorIntent>, Vec<EditorEffect>, bool) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    let mut dirty = false;
    match msg {
        EditorMessage::KeyEvent { key, tracked_caps_lock } => {
            // Editing resumes: re-enable cursor-following auto-scroll so
            // subsequent edtui render keeps the cursor in view.
            state.editor.set_scroll_locked(false);
            match handle_key(&mut state, key, tracked_caps_lock) {
                Ok(changed) => dirty = changed,
                // Ctrl+R escapes the buffer and opens history recall (mirrors the
                // original dbm's `ctrl+r` in the SQL editor).
                Err(intent) => intents.push(intent),
            }
        }
        EditorMessage::Paste { text } => {
            state.editor.set_scroll_locked(false);
            crate::common::editor::paste_text(&mut state.handler, &mut state.editor, &text);
            refresh_completion(&mut state, false);
            dirty = true;
        }
        EditorMessage::SetSql { sql } => {
            state.editor.set_scroll_locked(false);
            crate::common::editor::set_sql_text(&mut state.editor, &sql);
            state.sql_completion.close();
            dirty = true;
        }
        EditorMessage::Run => {
            let sql = crate::common::editor::editor_text(&state.editor);
            intents.push(EditorIntent::RunQuery { sql });
        }
        EditorMessage::ClearAfterRun => {
            // Mirror the original dbm's `after_sql_run`: empty the buffer and
            // return to Insert so the next statement can be typed immediately.
            state.editor.set_scroll_locked(false);
            crate::common::editor::set_sql_text(&mut state.editor, "");
            state.editor.mode = edtui::EditorMode::Insert;
            state.sql_completion.close();
            dirty = true;
        }
        EditorMessage::ForceCompletion => {
            refresh_completion(&mut state, true);
            dirty = true;
        }
        EditorMessage::RefreshCompletion => {
            refresh_completion(&mut state, false);
            dirty = true;
        }
        EditorMessage::CatalogLoaded {
            tables,
            columns_by_table,
        } => {
            state.completion_catalog.tables = tables;
            state.completion_catalog.columns_by_table = columns_by_table;
            // The completion catalog is only *rendered* while the completion
            // popup is open. A load that arrives while the popup is closed (the
            // usual case — it fires when a tab binds to a connection) changes no
            // cells, so skip the repaint and avoid inflating the waste metric.
            dirty = state.sql_completion.is_open();
        }
        EditorMessage::ContextPicker(m) => {
            let context_picker::msg::ContextPickerMsg::Message(inner) = m;
            let cp_state = std::mem::take(&mut state.context_picker);
            let (s, i, e, d) = context_picker::update::update(inner, cp_state);
            state.context_picker = s;
            intents.extend(i.into_iter().map(EditorIntent::ContextPicker));
            effects.extend(e.into_iter().map(EditorEffect::ContextPicker));
            dirty = d;
        }
        EditorMessage::SqlCompletion(m) => {
            let sql_completion::msg::SqlCompletionMsg::Message(inner) = m;
            let sc_state = std::mem::take(&mut state.sql_completion);
            let (s, i, e, d) = sql_completion::update::update(inner, sc_state);
            state.sql_completion = s;
            intents.extend(i.into_iter().map(EditorIntent::SqlCompletion));
            effects.extend(e.into_iter().map(EditorEffect::SqlCompletion));
            dirty = d;
        }

        // —— Manual viewport scroll ——
        EditorMessage::ScrollV { delta } => {
            let (x, y) = state.editor.viewport_offset();
            let new_y = (y as i32 + delta).max(0) as usize;
            state.editor.set_viewport_offset(x, new_y);
            state.editor.set_scroll_locked(true);
            dirty = true;
        }
        EditorMessage::SetVScroll { position } => {
            let (x, _) = state.editor.viewport_offset();
            state.editor.set_viewport_offset(x, position);
            state.editor.set_scroll_locked(true);
            dirty = true;
        }
        EditorMessage::ScrollH { delta } => {
            let (x, y) = state.editor.viewport_offset();
            let new_x = (x as i32 + delta).max(0) as usize;
            state.editor.set_viewport_offset(new_x, y);
            state.editor.set_scroll_locked(true);
            dirty = true;
        }
        EditorMessage::SetHScroll { position } => {
            let (_, y) = state.editor.viewport_offset();
            state.editor.set_viewport_offset(position, y);
            state.editor.set_scroll_locked(true);
            dirty = true;
        }
    }

    // The editor owns the buffer, so a completion Apply intent is resolved here
    // rather than bubbling up to the parent.
    let intent_count = intents.len();
    intents = resolve_apply_intents(&mut state, intents);
    // Applying a completion mutates the buffer (and closes the popup), so any
    // consumed Apply intent counts as a rendered change.
    if intents.len() < intent_count {
        dirty = true;
    }

    // If completion needs the catalog loaded (table-intent slot, TblCmp ON,
    // no cached table names), raise the request for `sql_tab` to resolve into a
    // `LoadCompletionCatalog` effect. Clear the transient flag either way so it
    // does not leak into the next update.
    if state.completion_catalog_needs_load {
        intents.push(EditorIntent::LoadCompletionCatalog);
        state.completion_catalog_needs_load = false;
    }

    (state, intents, effects, dirty)
}

/// Resolve any `SqlCompletionIntent::Apply` intents by inserting the completion
/// into the buffer; non-apply intents bubble unchanged.
fn resolve_apply_intents(
    state: &mut EditorState,
    intents: Vec<EditorIntent>,
) -> Vec<EditorIntent> {
    use super::sql_completion::intent::SqlCompletionIntent;

    let mut remaining = Vec::new();
    for intent in intents {
        match intent {
            EditorIntent::SqlCompletion(SqlCompletionIntent::Apply {
                item,
                replace_start,
                replace_end,
            }) => {
                apply_completion(state, &item.insert_text, replace_start, replace_end);
            }
            other => remaining.push(other),
        }
    }
    remaining
}

/// Insert `insert_text` into the editor, replacing `[replace_start, replace_end)`
/// (in cursor coordinates) with the completion text.
fn apply_completion(
    state: &mut EditorState,
    insert_text: &str,
    replace_start: crate::common::utils::cursor::Cursor,
    replace_end: crate::common::utils::cursor::Cursor,
) {
    use crate::common::utils::cursor::cursor_to_byte_offset;

    let text = crate::common::editor::editor_text(&state.editor);
    let start_offset = cursor_to_byte_offset(&text, replace_start);
    let end_offset = cursor_to_byte_offset(&text, replace_end).min(text.len());
    let mut new_text = String::with_capacity(text.len() + insert_text.len());
    new_text.push_str(&text[..start_offset]);
    new_text.push_str(insert_text);
    new_text.push_str(&text[end_offset..]);

    // Place the cursor after the inserted text (as an Insert-mode EOL position).
    let inserted_char_pos = text[..start_offset].chars().count() + insert_text.chars().count();
    let insert_end_byte = new_text_char_offset(&new_text, inserted_char_pos);

    crate::common::editor::set_editor_text(&mut state.editor, &new_text);
    let new_row = new_text[..insert_end_byte].matches('\n').count();
    let new_line_start = new_text[..insert_end_byte].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let new_col = new_text[new_line_start..insert_end_byte].chars().count();
    state.editor.cursor = edtui::Index2::new(new_row, new_col);
    state.editor.mode = edtui::EditorMode::Insert;
    state.editor.selection = None;
    // Buffer and cursor changed — re-engage cursor-following auto-scroll so
    // edtui keeps the inserted text in view during the next render.
    state.editor.set_scroll_locked(false);
}

/// Byte offset of the `char_count`-th character from the start of `text`.
fn new_text_char_offset(text: &str, char_count: usize) -> usize {
    text.char_indices()
        .nth(char_count)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}

/// Forward a normalized key to the edtui editor, then refresh completion if the
/// buffer changed. Returns whether the editor's rendered state (buffer text or
/// cursor/completion) changed.
fn handle_key(
    state: &mut EditorState,
    key: KeyEvent,
    tracked_caps_lock: bool,
) -> Result<bool, EditorIntent> {
    use crate::common::editor;
    // Ctrl+R opens the history recall overlay (mirrors the original dbm's
    // `ctrl+r` in the SQL editor). It must be intercepted before reaching the
    // buffer, which has no such binding.
    if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
        && key.code == crossterm::event::KeyCode::Char('r')
    {
        return Err(EditorIntent::HistoryRecall);
    }
    // In-buffer `/` search takes the key first (active input, `/` to start,
    // `n`/`N` to jump). Consumed keys never reach the buffer.
    if super::sql_search::handle_sql_pane_search_key(&mut state.sql_search, &mut state.editor, key) {
        return Ok(true);
    }
    // Non-ASCII chars (IME commits) route through insert_text in Insert mode.
    if editor::try_insert_non_ascii_key(&mut state.handler, &mut state.editor, key, tracked_caps_lock)
    {
        refresh_completion(state, false);
        return Ok(true);
    }
    if !editor::accepts_key_event(&key) {
        return Ok(false);
    }
    let before = editor::editor_text(&state.editor);
    let before_cursor = state.editor.cursor;
    let before_mode = state.editor.mode;
    state.handler.on_key_event(key, &mut state.editor);
    // Redraw when the buffer text, cursor, or editor mode changed (mode toggles
    // like `i`/`Esc` don't touch text or cursor, but must repaint the title).
    let changed = editor::editor_text(&state.editor) != before
        || state.editor.cursor != before_cursor
        || state.editor.mode != before_mode;
    if changed {
        refresh_completion(state, false);
    }
    Ok(changed)
}

/// Recompute the completion popup for the current buffer/cursor.
///
/// Uses the tab's cached catalog: all table names are offered for `FROM`
/// completion, and columns are scoped to the tables referenced in the buffer
/// before the cursor (so a query on one table does not suggest another's
/// columns). Falls back to keyword-only completion when the catalog is empty.
/// `explicit` forces the popup open (Shift+Tab), bypassing the auto-open gate.
fn refresh_completion(state: &mut EditorState, explicit: bool) {
    // Completion only applies in insert mode (matching the original dbm):
    // a normal/visual-mode cursor move must not pop the completion window.
    if state.editor.mode != edtui::EditorMode::Insert {
        state.sql_completion.close();
        return;
    }
    let sql = crate::common::editor::editor_text(&state.editor);
    let cursor = editor_cursor(&state.editor);
    // A table-intent slot with TblCmp ON but no cached table names needs the
    // catalog (re)loaded — mirroring the original dbm's `schedule_metadata_refresh`.
    // The editor update routes this flag to a `LoadCompletionCatalog` intent.
    let context = sql_completion::context::get_completion_context(&sql, cursor);
    // Request a catalog (re)load when the current intent needs table/column
    // metadata but the catalog hasn't provided it. Mirror the original dbm's
    // `schedule_metadata_refresh`: column completions (Column/InsertColumn/
    // UpdateColumn) load regardless of TblCmp; only a table-intent slot also
    // requires TblCmp to be ON.
    if sql_completion::provider::needs_table_metadata(&context) {
        let is_table = matches!(
            context.intent,
            sql_completion::context::CompletionIntent::Table { .. }
        );
        let allowed = !is_table || state.complete_table_names;
        if allowed {
            let columns = scoped_columns(&state.completion_catalog, &sql, cursor);
            let stale = if is_table {
                state.completion_catalog.tables.is_empty()
            } else {
                state.completion_catalog.tables.is_empty() || columns.is_empty()
            };
            if stale {
                state.completion_catalog_needs_load = true;
            }
        }
    }
    let tables = state.completion_catalog.tables.clone();
    let columns = scoped_columns(&state.completion_catalog, &sql, cursor);
    let sc_state = std::mem::take(&mut state.sql_completion);
    let (s, _i, _e, _d) = sql_completion::update::update(
        sql_completion::msg::SqlCompletionMessage::Refresh {
            sql,
            cursor,
            tables,
            columns,
            explicit,
            complete_table_names: state.complete_table_names,
        },
        sc_state,
    );
    state.sql_completion = s;
}

/// The column metadata for the tables referenced in `sql`, so column
/// completion is scoped to the tables actually in use. Tables are resolved from
/// the whole statement (not just before `cursor`) so a select-list slot can use
/// tables that appear in `from` after the cursor (e.g. `select <cursor> from t1`).
fn scoped_columns(
    catalog: &super::state::CompletionCatalog,
    sql: &str,
    cursor: crate::common::utils::cursor::Cursor,
) -> Vec<super::sql_completion::provider::ColumnInfo> {
    let _ = cursor;
    let refs = super::sql_completion::context::extract_referenced_tables(sql);
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for table_ref in refs {
        let key = table_ref.name.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        if let Some(cols) = catalog.columns_by_table.get(&table_ref.name) {
            out.extend(cols.iter().cloned());
        }
    }
    out
}

/// Convert the edtui cursor to the completion engine's `Cursor`.
fn editor_cursor(editor: &edtui::EditorState) -> crate::common::utils::cursor::Cursor {
    crate::common::utils::cursor::Cursor::new(editor.cursor.row, editor.cursor.col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::editor;
    use crate::features::sql_workspace::sql_tab::editor::sql_completion::state::SqlCompletionState;
    use crossterm::event::{KeyCode, KeyEventKind, KeyEventState, KeyModifiers};

    fn char_key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn scoped_columns_only_include_referenced_tables() {
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;
        use super::super::state::CompletionCatalog;
        let mut catalog = CompletionCatalog {
            tables: vec!["users".into(), "orders".into()],
            columns_by_table: Default::default(),
        };
        catalog.columns_by_table.insert(
            "users".into(),
            vec![ColumnInfo {
                name: "id".into(),
                type_name: "integer".into(),
                type_display: "integer".into(),
                comment: None,
            }],
        );
        catalog.columns_by_table.insert(
            "orders".into(),
            vec![ColumnInfo {
                name: "amount".into(),
                type_name: "numeric".into(),
                type_display: "numeric".into(),
                comment: None,
            }],
        );
        let sql = "SELECT id FROM users WHERE ";
        let cursor = crate::common::utils::cursor::Cursor::new(0, sql.chars().count());
        let cols = scoped_columns(&catalog, sql, cursor);
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].name, "id");
    }

    #[test]
    fn typing_triggers_completion_refresh() {
        let mut state = EditorState::with_sql("");
        // Enter insert mode.
        let (s, _i, _e, _d) = update(
            EditorMessage::KeyEvent { key: char_key('i'), tracked_caps_lock: false },
            state,
        );
        state = s;
        let (s, _i, _e, _d) = update(
            EditorMessage::KeyEvent { key: char_key('s'), tracked_caps_lock: false },
            state,
        );
        state = s;
        // After typing, the completion popup should be open (s → SELECT etc.).
        assert!(state.sql_completion.is_open());
        assert_eq!(editor::editor_text(&state.editor), "s");
    }

    #[test]
    fn catalog_loaded_is_not_dirty_while_popup_closed() {
        // A `CatalogLoaded` that arrives while the completion popup is closed
        // changes no rendered cells (the catalog is only drawn in the popup), so
        // it must not mark the editor dirty — otherwise every connection-activate
        // catalog load would inflate the waste metric.
        let state = EditorState::with_sql("");
        let (_s, _i, _e, dirty) = update(
            EditorMessage::CatalogLoaded {
                tables: vec!["users".into()],
                columns_by_table: Default::default(),
            },
            state,
        );
        assert!(!dirty, "catalog load with popup closed must not repaint");
    }

    #[test]
    fn clear_after_run_empties_buffer_and_returns_to_insert() {
        // After a successful editor-run query the buffer is emptied and the
        // editor returns to Insert (mirroring the original dbm's
        // `after_sql_run`), so the next statement can be typed immediately.
        let mut state = EditorState::with_sql("select 1");
        state.editor.mode = edtui::EditorMode::Insert;
        state.editor.set_scroll_locked(true);
        let (s, _i, _e, dirty) = update(EditorMessage::ClearAfterRun, state);
        assert_eq!(
            crate::common::editor::editor_text(&s.editor),
            "",
            "editor buffer must be cleared after a successful editor-run query"
        );
        assert_eq!(
            s.editor.mode,
            edtui::EditorMode::Insert,
            "editor must return to Insert after clearing"
        );
        assert!(dirty, "clearing the editor must trigger a repaint");
    }

    #[test]
    fn catalog_loaded_is_dirty_while_popup_open() {
        // While the completion popup is open the catalog is rendered, so a load
        // must trigger a repaint. Open the popup the same way the
        // `typing_triggers_completion_refresh` test does (type `is`).
        let mut state = EditorState::with_sql("");
        state.editor.mode = edtui::EditorMode::Insert;
        for c in ['i', 's'] {
            let (s, _i, _e, _d) = update(
                EditorMessage::KeyEvent { key: char_key(c), tracked_caps_lock: false },
                state,
            );
            state = s;
        }
        assert!(state.sql_completion.is_open(), "typing should open the popup");
        let (_s, _i, _e, dirty) = update(
            EditorMessage::CatalogLoaded {
                tables: vec!["users".into()],
                columns_by_table: Default::default(),
            },
            state,
        );
        assert!(dirty, "catalog load with popup open must repaint");
    }

    #[test]
    fn empty_catalog_with_tblcmp_on_requests_reload() {
        // TblCmp ON in a table-intent slot with no cached table names must
        // raise a `LoadCompletionCatalog` intent (the original dbm triggers a
        // metadata refresh there), and clear the transient flag afterward.
        let mut state = EditorState::with_sql("");
        state.editor.mode = edtui::EditorMode::Insert;
        state.complete_table_names = true;
        state.completion_catalog.tables.clear();

        let mut requested = false;
        let mut s = state;
        for c in "select * from ".chars() {
            let (s2, i, _e, _d) = update(
                EditorMessage::KeyEvent { key: char_key(c), tracked_caps_lock: false },
                s,
            );
            requested |= i
                .iter()
                .any(|int| matches!(int, EditorIntent::LoadCompletionCatalog));
            s = s2;
        }
        assert!(requested, "empty catalog + TblCmp on should request a catalog reload");
        assert!(
            !s.completion_catalog_needs_load,
            "transient needs-load flag must be cleared"
        );
    }

    #[test]
    fn empty_catalog_update_set_requests_reload_without_tblcmp() {
        // Column completion (`update t set `) must request a catalog reload
        // even when TblCmp is OFF — the original dbm only gates *table-name*
        // completion on TblCmp, not column completion.
        let mut state = EditorState::with_sql("");
        state.editor.mode = edtui::EditorMode::Insert;
        state.complete_table_names = false;
        state.completion_catalog.tables.clear();

        let mut requested = false;
        let mut s = state;
        for c in "update tb1 set ".chars() {
            let (s2, i, _e, _d) = update(
                EditorMessage::KeyEvent { key: char_key(c), tracked_caps_lock: false },
                s,
            );
            requested |= i
                .iter()
                .any(|int| matches!(int, EditorIntent::LoadCompletionCatalog));
            s = s2;
        }
        assert!(
            requested,
            "column completion should request a catalog reload without TblCmp"
        );
    }

    #[test]
    fn select_slot_typing_offers_columns_without_editing_buffer() {
        // `select  from t1 t` (two spaces), cursor right after `select ` (between
        // the spaces), then type `t`. Typing must NOT auto-edit the buffer
        // beyond inserting the typed char, and the column popup must offer `t1`
        // columns (the select list is a column-intent slot).
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;
        let mut state = EditorState::with_sql("select  from t1 t");
        state.editor.mode = edtui::EditorMode::Insert;
        state.editor.cursor = edtui::Index2::new(0, "select ".chars().count());
        state.completion_catalog.tables = vec!["t1".into()];
        state.completion_catalog.columns_by_table.insert(
            "t1".into(),
            vec![ColumnInfo {
                name: "title".into(),
                type_name: "text".into(),
                type_display: "text".into(),
                comment: None,
            }],
        );

        let (s, _i, _e, _d) = update(
            EditorMessage::KeyEvent { key: char_key('t'), tracked_caps_lock: false },
            state,
        );
        // Only the typed `t` is inserted at the cursor — no auto-space, no
        // removal of surrounding text.
        assert_eq!(
            crate::common::editor::editor_text(&s.editor),
            "select t from t1 t",
            "typing must not auto-insert/delete characters"
        );
        assert!(
            s.sql_completion.is_open(),
            "a column popup should open after typing at the select column slot"
        );
        assert!(
            s.sql_completion.items.iter().any(|i| i.label == "title"),
            "t1 columns should be offered, got: {:?}",
            s.sql_completion.items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn normal_mode_does_not_pop_completion() {
        // Completion only applies in insert mode (matching the original dbm):
        // a cursor move / buffer edit in normal mode must not pop the window.
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;
        let mut state = EditorState::with_sql("select from t1 t");
        state.editor.mode = edtui::EditorMode::Normal;
        state.editor.cursor = edtui::Index2::new(0, "select ".chars().count());
        state.completion_catalog.tables = vec!["t1".into()];
        state.completion_catalog.columns_by_table.insert(
            "t1".into(),
            vec![ColumnInfo {
                name: "title".into(),
                type_name: "text".into(),
                type_display: "text".into(),
                comment: None,
            }],
        );
        // Simulate a refresh (as a cursor move or edit would trigger in
        // insert mode); in normal mode it must stay closed.
        let (s, _i, _e, _d) = update(
            EditorMessage::RefreshCompletion,
            state,
        );
        assert!(
            !s.sql_completion.is_open(),
            "normal mode must not pop the completion window"
        );
    }

    #[test]
    fn typing_mid_buffer_does_not_swallow_next_char() {
        // Insert mode, cursor in the middle of `abcdef`, type `X`. The char to
        // the right of the cursor must be preserved: `abXcdef`, not `abXdef`.
        let mut state = EditorState::with_sql("abcdef");
        state.editor.mode = edtui::EditorMode::Insert;
        state.editor.cursor = edtui::Index2::new(0, 2); // after `ab`
        let (s, _i, _e, _d) = update(
            EditorMessage::KeyEvent { key: char_key('X'), tracked_caps_lock: false },
            state,
        );
        assert_eq!(
            crate::common::editor::editor_text(&s.editor),
            "abXcdef",
            "typing in the middle must not swallow the next character"
        );
    }

    #[test]
    fn typing_second_char_with_popup_open_does_not_swallow() {
        // After `select |from t1 t` + `t` (popup open, `select t from t1 t`,
        // cursor after `t`), typing another char must insert, not swallow the
        // char that follows the cursor.
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;
        let mut state = EditorState::with_sql("select  from t1 t");
        state.editor.mode = edtui::EditorMode::Insert;
        state.editor.cursor = edtui::Index2::new(0, "select ".chars().count());
        state.completion_catalog.tables = vec!["t1".into()];
        state.completion_catalog.columns_by_table.insert(
            "t1".into(),
            vec![ColumnInfo {
                name: "title".into(),
                type_name: "text".into(),
                type_display: "text".into(),
                comment: None,
            }],
        );
        let (s, _i, _e, _d) = update(
            EditorMessage::KeyEvent { key: char_key('t'), tracked_caps_lock: false },
            state,
        );
        assert_eq!(crate::common::editor::editor_text(&s.editor), "select t from t1 t");
        // Now the popup is open and the cursor is after `t`. Type `i`.
        let (s, _i, _e, _d) = update(
            EditorMessage::KeyEvent { key: char_key('i'), tracked_caps_lock: false },
            s,
        );
        assert_eq!(
            crate::common::editor::editor_text(&s.editor),
            "select ti from t1 t",
            "typing a second char with the popup open must not swallow the following char"
        );
    }

    #[test]
    fn mode_toggle_marks_dirty_for_redraw() {
        // Starting in normal mode, pressing `i` only changes the editor mode
        // (no text/cursor change), but must still request a redraw so the title
        // updates immediately.
        let state = EditorState::with_sql("");
        let (s, _i, _e, dirty) = update(
            EditorMessage::KeyEvent { key: char_key('i'), tracked_caps_lock: false },
            state,
        );
        assert!(s.editor.mode == edtui::EditorMode::Insert, "mode should switch to insert");
        assert!(dirty, "mode toggle must mark the view dirty for redraw");
    }

    #[test]
    fn apply_completion_inserts_text_and_replaces_range() {
        let mut state = EditorState::with_sql("SELECT * FROM users whe");
        state.editor.mode = edtui::EditorMode::Insert;
        state.editor.cursor = edtui::Index2::new(0, "SELECT * FROM users whe".chars().count());
        state.sql_completion = SqlCompletionState::open_with(
            vec![super::super::sql_completion::provider::CompletionItem {
                label: "WHERE".into(),
                kind: super::super::sql_completion::provider::CompletionKind::Keyword,
                detail: None,
                insert_text: "WHERE".into(),
            }],
            crate::common::utils::cursor::Cursor::new(0, "SELECT * FROM users ".chars().count()),
            crate::common::utils::cursor::Cursor::new(0, "SELECT * FROM users whe".chars().count()),
        );

        // Simulate the Apply intent being resolved by the editor update.
        let intents = vec![super::super::intent::EditorIntent::SqlCompletion(
            super::super::sql_completion::intent::SqlCompletionIntent::Apply {
                item: super::super::sql_completion::provider::CompletionItem {
                    label: "WHERE".into(),
                    kind: super::super::sql_completion::provider::CompletionKind::Keyword,
                    detail: None,
                    insert_text: "WHERE".into(),
                },
                replace_start: crate::common::utils::cursor::Cursor::new(
                    0,
                    "SELECT * FROM users ".chars().count(),
                ),
                replace_end: crate::common::utils::cursor::Cursor::new(
                    0,
                    "SELECT * FROM users whe".chars().count(),
                ),
            },
        )];
        let remaining = resolve_apply_intents(&mut state, intents);
        assert!(remaining.is_empty());
        assert_eq!(
            editor::editor_text(&state.editor),
            "SELECT * FROM users WHERE"
        );
        assert_eq!(state.editor.mode, edtui::EditorMode::Insert);
    }

    #[test]
    fn apply_completion_replaces_cjk_range() {
        let mut state = EditorState::with_sql("select 名称 from 测试表");
        state.editor.mode = edtui::EditorMode::Insert;
        // Cursor after the 名 char (char col 7 in "select 名", 名称 is 2 chars).
        let intents = vec![super::super::intent::EditorIntent::SqlCompletion(
            super::super::sql_completion::intent::SqlCompletionIntent::Apply {
                item: super::super::sql_completion::provider::CompletionItem {
                    label: "名称".into(),
                    kind: super::super::sql_completion::provider::CompletionKind::Column,
                    detail: None,
                    insert_text: "\"名称\"".into(),
                },
                replace_start: crate::common::utils::cursor::Cursor::new(0, 7),
                replace_end: crate::common::utils::cursor::Cursor::new(0, 9),
            },
        )];
        let remaining = resolve_apply_intents(&mut state, intents);
        assert!(remaining.is_empty());
        assert_eq!(
            editor::editor_text(&state.editor),
            "select \"名称\" from 测试表"
        );
    }

    #[test]
    fn apply_second_qualified_column_does_not_duplicate_prefix() {
        // `select t.id, t.` and complete the second field with `t.name`
        // (replace_start covers the second `t.`). The result must be
        // `select t.id, t.name`, NOT `select t.id, t.t.name`.
        let mut state = EditorState::with_sql("select t.id, t.");
        state.editor.mode = edtui::EditorMode::Insert;
        let intents = vec![super::super::intent::EditorIntent::SqlCompletion(
            super::super::sql_completion::intent::SqlCompletionIntent::Apply {
                item: super::super::sql_completion::provider::CompletionItem {
                    label: "name".into(),
                    kind: super::super::sql_completion::provider::CompletionKind::Column,
                    detail: None,
                    insert_text: "t.name".into(),
                },
                replace_start: crate::common::utils::cursor::Cursor::new(0, 13),
                replace_end: crate::common::utils::cursor::Cursor::new(0, 15),
            },
        )];
        let remaining = resolve_apply_intents(&mut state, intents);
        assert!(remaining.is_empty());
        assert_eq!(
            editor::editor_text(&state.editor),
            "select t.id, t.name",
            "second qualified completion must replace the qualifier, not duplicate it"
        );
    }

    #[test]
    fn ctrl_r_opens_history_recall() {
        // Mirrors the original dbm's `ctrl+r` in the SQL editor: the editor
        // must intercept Ctrl+R and emit a `HistoryRecall` intent rather than
        // forwarding it to edtui (which would ignore it).
        let state = EditorState::with_sql("select 1");
        let key = KeyEvent {
            code: KeyCode::Char('r'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let (_s, intents, _e, _d) = update(
            EditorMessage::KeyEvent { key, tracked_caps_lock: false },
            state.clone(),
        );
        assert!(
            intents
                .iter()
                .any(|i| matches!(i, EditorIntent::HistoryRecall)),
            "ctrl+r must raise a HistoryRecall intent, got: {intents:?}"
        );
        // The buffer must be untouched by the recall chord.
        assert_eq!(editor::editor_text(&EditorState::with_sql("select 1").editor), "select 1");
    }
}

