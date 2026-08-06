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
) -> (EditorState, Vec<EditorIntent>, Vec<EditorEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    match msg {
        EditorMessage::KeyEvent { key, tracked_caps_lock } => {
            handle_key(&mut state, key, tracked_caps_lock);
        }
        EditorMessage::Paste { text } => {
            crate::common::editor::paste_text(&mut state.handler, &mut state.editor, &text);
            refresh_completion(&mut state);
        }
        EditorMessage::SetSql { sql } => {
            crate::common::editor::set_sql_text(&mut state.editor, &sql);
            state.sql_completion.close();
        }
        EditorMessage::Run => {
            let sql = crate::common::editor::editor_text(&state.editor);
            intents.push(EditorIntent::RunQuery { sql });
        }
        EditorMessage::ContextPicker(m) => {
            let context_picker::msg::ContextPickerMsg::Message(inner) = m;
            let cp_state = std::mem::take(&mut state.context_picker);
            let (s, i, e) = context_picker::update::update(inner, cp_state);
            state.context_picker = s;
            intents.extend(i.into_iter().map(EditorIntent::ContextPicker));
            effects.extend(e.into_iter().map(EditorEffect::ContextPicker));
        }
        EditorMessage::SqlCompletion(m) => {
            let sql_completion::msg::SqlCompletionMsg::Message(inner) = m;
            let sc_state = std::mem::take(&mut state.sql_completion);
            let (s, i, e) = sql_completion::update::update(inner, sc_state);
            state.sql_completion = s;
            intents.extend(i.into_iter().map(EditorIntent::SqlCompletion));
            effects.extend(e.into_iter().map(EditorEffect::SqlCompletion));
        }
    }

    // The editor owns the buffer, so a completion Apply intent is resolved here
    // rather than bubbling up to the parent.
    intents = resolve_apply_intents(&mut state, intents);

    (state, intents, effects)
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
}

/// Byte offset of the `char_count`-th character from the start of `text`.
fn new_text_char_offset(text: &str, char_count: usize) -> usize {
    text.char_indices()
        .nth(char_count)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}

/// Forward a normalized key to the edtui editor, then refresh completion if the
/// buffer changed.
fn handle_key(state: &mut EditorState, key: KeyEvent, tracked_caps_lock: bool) {
    use crate::common::editor;
    // In-buffer `/` search takes the key first (active input, `/` to start,
    // `n`/`N` to jump). Consumed keys never reach the buffer.
    if super::sql_search::handle_sql_pane_search_key(&mut state.sql_search, &mut state.editor, key) {
        return;
    }
    // Non-ASCII chars (IME commits) route through insert_text in Insert mode.
    if editor::try_insert_non_ascii_key(&mut state.handler, &mut state.editor, key, tracked_caps_lock)
    {
        refresh_completion(state);
        return;
    }
    if !editor::accepts_key_event(&key) {
        return;
    }
    let before = editor::editor_text(&state.editor);
    state.handler.on_key_event(key, &mut state.editor);
    if editor::editor_text(&state.editor) != before {
        refresh_completion(state);
    }
}

/// Recompute the completion popup for the current buffer/cursor.
///
/// Metadata (tables/columns) is passed empty for now; the tab/session
/// integration will supply cached catalog metadata once wired.
fn refresh_completion(state: &mut EditorState) {
    let sql = crate::common::editor::editor_text(&state.editor);
    let cursor = editor_cursor(&state.editor);
    let sc_state = std::mem::take(&mut state.sql_completion);
    let (s, _i, _e) = sql_completion::update::update(
        sql_completion::msg::SqlCompletionMessage::Refresh {
            sql,
            cursor,
            tables: Vec::new(),
            columns: Vec::new(),
        },
        sc_state,
    );
    state.sql_completion = s;
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
    fn typing_triggers_completion_refresh() {
        let mut state = EditorState::with_sql("");
        // Enter insert mode.
        let (s, _i, _e) = update(
            EditorMessage::KeyEvent { key: char_key('i'), tracked_caps_lock: false },
            state,
        );
        state = s;
        let (s, _i, _e) = update(
            EditorMessage::KeyEvent { key: char_key('s'), tracked_caps_lock: false },
            state,
        );
        state = s;
        // After typing, the completion popup should be open (s → SELECT etc.).
        assert!(state.sql_completion.is_open());
        assert_eq!(editor::editor_text(&state.editor), "s");
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
}

