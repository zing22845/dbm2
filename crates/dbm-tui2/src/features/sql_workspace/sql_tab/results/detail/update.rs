//! Results detail sub-module update.

use super::super::detail_edit::detail_draft_dirty;
use super::effect::DetailEffect;
use super::intent::DetailIntent;
use super::msg::DetailMessage;
use super::state::DetailState;
use crate::common::editor;
use crate::features::sql_workspace::sql_tab::editor::mouse::MouseGestureOutcome;

/// Run `key` against the focused detail editor. Returns `true` when the editor
/// changed (buffer text, cursor, mode, or selection) and thus needs a repaint.
///
/// Mirrors the SQL editor's `handle_key`: non-ASCII IME commits route through
/// `insert_text`, only keys edtui understands are forwarded, and after every
/// accepted key the draft/dirty state and baseline-diff highlights are
/// re-synced from the buffer.
fn apply_editor_key(
    state: &mut DetailState,
    key: crossterm::event::KeyEvent,
    tracked_caps_lock: bool,
) -> bool {
    let Some(host) = state.editor.as_mut() else {
        return false;
    };
    host.editor.set_scroll_locked(false);
    if editor::try_insert_non_ascii_key(&mut host.handler, &mut host.editor, key, tracked_caps_lock)
    {
        state.sync_draft_from_editor();
        return true;
    }
    if !editor::accepts_key_event(&key) {
        return false;
    }
    let before_text = editor::editor_text(&host.editor);
    let before_cursor = host.editor.cursor;
    let before_mode = host.editor.mode;
    let before_selection = host.editor.selection.clone();
    host.handler.on_key_event(key, &mut host.editor);
    let changed = editor::editor_text(&host.editor) != before_text
        || host.editor.cursor != before_cursor
        || host.editor.mode != before_mode
        || host.editor.selection != before_selection;
    if changed {
        state.sync_draft_from_editor();
    }
    changed
}

/// Apply a decoded mouse gesture (cursor / mode / selection) to the focused
/// editor. Refused when no editor is focused. The buffer text is untouched, so
/// this never makes the draft dirty — it only moves the caret / selects text so
/// the copy key works. Returns `true` (a repaint happened).
fn apply_mouse_gesture(state: &mut DetailState, outcome: MouseGestureOutcome) -> bool {
    let Some(host) = state.editor.as_mut() else {
        return false;
    };
    if !state.focused {
        return false;
    }
    host.editor.set_scroll_locked(false);
    host.editor.cursor = outcome.cursor;
    host.editor.mode = outcome.mode;
    host.editor.selection = outcome.selection;
    true
}

pub fn update(
    msg: DetailMessage,
    mut state: DetailState,
) -> (DetailState, Vec<DetailIntent>, Vec<DetailEffect>, bool) {
    let intents = Vec::new();
    let effects = Vec::new();
    let dirty = match msg {
        DetailMessage::Scroll { delta } => {
            let before = state.scroll;
            if delta > 0 {
                state.scroll = state.scroll.saturating_add(delta as usize);
            } else {
                state.scroll = state.scroll.saturating_sub(delta.unsigned_abs() as usize);
            }
            state.scroll != before
        }
        DetailMessage::SetDraft { text } => {
            state.draft = text.clone();
            state.dirty = detail_draft_dirty(&text, &state.baseline);
            true
        }
        DetailMessage::LoadCell { value } => {
            state.load_cell(&value);
            true
        }
        DetailMessage::ClearDraft => {
            state.clear_draft();
            true
        }
        DetailMessage::KeyEvent {
            key,
            tracked_caps_lock,
        } => apply_editor_key(&mut state, key, tracked_caps_lock),
        // Apply the edtui-produced cursor/mode/selection from a click / drag /
        // release. The buffer text is untouched, so this can never make the
        // draft dirty — it only moves the caret or selects text for the copy key.
        DetailMessage::MouseGesture { outcome } => apply_mouse_gesture(&mut state, outcome),
    };
    (state, intents, effects, dirty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn press(code: KeyCode) -> crossterm::event::KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn char_key(c: char) -> crossterm::event::KeyEvent {
        press(KeyCode::Char(c))
    }

    fn focused_state(value: &str) -> DetailState {
        let mut s = DetailState::default();
        s.focus_editor(value);
        s
    }

    /// Place the focused editor into Insert mode with the cursor at end-of-line
    /// (like the SQL editor after `ClearAfterRun`), so typing appends.
    fn insert_at_eol(mut s: DetailState) -> DetailState {
        let host = s.editor.as_mut().expect("focused editor exists");
        host.editor.mode = edtui::EditorMode::Insert;
        crate::common::editor::move_cursor_to_eol(&mut host.editor);
        s
    }

    #[test]
    fn key_when_not_focused_is_ignored() {
        let s = DetailState::default();
        let (s2, _i, _e, dirty) = update(
            DetailMessage::KeyEvent {
                key: char_key('x'),
                tracked_caps_lock: false,
            },
            s,
        );
        assert!(!dirty);
        assert!(s2.editor.is_none());
    }

    #[test]
    fn typing_edits_draft_and_marks_dirty() {
        let s = insert_at_eol(focused_state("abc"));
        let (s2, _i, _e, d) = update(
            DetailMessage::KeyEvent {
                key: char_key('d'),
                tracked_caps_lock: false,
            },
            s,
        );
        assert!(d, "inserting into the draft must repaint");
        assert_eq!(s2.draft, "abcd");
        assert!(s2.dirty, "draft differs from baseline");
        assert!(
            !s2.editor.as_ref().unwrap().editor.highlights.is_empty(),
            "dirty draft must carry baseline-diff highlights"
        );
    }

    #[test]
    fn normal_mode_start_enters_insert_via_i() {
        // The focused detail editor starts in Normal (like the SQL editor and
        // the original dbm); pressing `i` switches to Insert. Note edtui's vim
        // semantics keep the cursor on the last character, so `i` then typing
        // inserts *before* it — verified here to document the behavior.
        let mut s = focused_state("abc");
        assert_eq!(
            s.editor.as_ref().unwrap().editor.mode,
            edtui::EditorMode::Normal,
            "a focused detail editor starts in normal (i to edit)"
        );
        let (s2, _i, _e, _d) = update(
            DetailMessage::KeyEvent {
                key: char_key('i'),
                tracked_caps_lock: false,
            },
            s,
        );
        s = s2;
        assert_eq!(
            s.editor.as_ref().unwrap().editor.mode,
            edtui::EditorMode::Insert
        );
        let (s3, _i, _e, d) = update(
            DetailMessage::KeyEvent {
                key: char_key('d'),
                tracked_caps_lock: false,
            },
            s,
        );
        assert!(d);
        assert_eq!(
            s3.draft, "abdc",
            "i inserts before the character under the cursor (vim)"
        );
    }

    #[test]
    fn esc_downgrades_insert_to_normal_without_clearing_editor() {
        let s = insert_at_eol(focused_state("abc"));
        assert_eq!(
            s.editor.as_ref().unwrap().editor.mode,
            edtui::EditorMode::Insert
        );
        let (s2, _i, _e, d) = update(
            DetailMessage::KeyEvent {
                key: press(KeyCode::Esc),
                tracked_caps_lock: false,
            },
            s,
        );
        assert!(d, "Esc from insert to normal must repaint");
        assert!(
            s2.editor.is_some(),
            "Esc must keep the editor (leaving detail is decided upstream)"
        );
        assert_eq!(
            s2.editor.as_ref().unwrap().editor.mode,
            edtui::EditorMode::Normal,
            "Esc lowers insert to normal"
        );
    }

    #[test]
    fn load_cell_resets_draft_and_baseline() {
        let mut s = focused_state("abc");
        s.dirty = true;
        let (s2, _i, _e, d) = update(
            DetailMessage::LoadCell {
                value: "xyz".into(),
            },
            s,
        );
        assert!(d);
        assert_eq!(s2.draft, "xyz");
        assert_eq!(s2.baseline, "xyz");
        assert!(!s2.dirty);
    }

    #[test]
    fn mouse_gesture_moves_caret_without_editing_draft() {
        let s = focused_state("abc");
        // A click outcome: place the caret at line 0, col 2 (middle of "abc").
        let outcome = MouseGestureOutcome {
            cursor: edtui::Index2::new(0, 2),
            mode: edtui::EditorMode::Normal,
            selection: None,
        };
        let (s2, _i, _e, dirty) = update(DetailMessage::MouseGesture { outcome }, s);
        assert!(dirty, "a caret move must repaint");
        assert_eq!(
            s2.editor.as_ref().unwrap().editor.cursor,
            edtui::Index2::new(0, 2)
        );
        assert!(!s2.dirty, "mouse gestures never edit the buffer");
        assert_eq!(s2.draft, "abc");
    }

    #[test]
    fn mouse_gesture_selection_keeps_draft_text() {
        let mut s = focused_state("select_me");
        s.dirty = true; // keep the dirty diff markers on the buffer
        let outcome = MouseGestureOutcome {
            cursor: edtui::Index2::new(0, 9),
            mode: edtui::EditorMode::Normal,
            selection: Some(edtui::Selection::new(
                edtui::Index2::new(0, 0),
                edtui::Index2::new(0, 9),
            )),
        };
        let (s2, _i, _e, dirty) = update(DetailMessage::MouseGesture { outcome }, s);
        assert!(dirty);
        assert!(
            s2.editor.as_ref().unwrap().editor.selection.is_some(),
            "selection must be applied for the copy key"
        );
        assert_eq!(s2.draft, "select_me", "gestures never alter the draft");
    }

    #[test]
    fn mouse_gesture_ignored_when_not_focused() {
        let s = DetailState::default();
        let outcome = MouseGestureOutcome {
            cursor: edtui::Index2::new(0, 0),
            mode: edtui::EditorMode::Normal,
            selection: None,
        };
        let (s2, _i, _e, dirty) = update(DetailMessage::MouseGesture { outcome }, s);
        assert!(!dirty);
        assert!(s2.editor.is_none());
    }
}
