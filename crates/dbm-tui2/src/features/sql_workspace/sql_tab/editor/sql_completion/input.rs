//! Key mapping for the completion popup while it is open.
//!
//! While the popup is open it captures a small key set (arrows, bare
//! Enter/Tab, Esc) and lets everything else fall through to the editor buffer.
//! Keeping the map in the feature's own `input` layer (like the instance
//! workspace) means the shell dispatcher never reaches into the view for key
//! handling.

use crossterm::event::KeyEvent;

/// Map a key to a completion message when the popup is open. Only the arrow
/// keys move the selection (matching the original dbm's `handle_popup_key`);
/// `j`/`k` are NOT bound here so they fall through to the editor buffer and can
/// be typed. A bare Enter/Tab applies the highlighted item; a bare Esc closes
/// the popup. Returns `None` when the key should fall through to the editor.
pub fn key_to_msg(key: KeyEvent) -> Option<super::msg::SqlCompletionMessage> {
    use super::msg::SqlCompletionMessage;
    use crossterm::event::{KeyCode, KeyModifiers};
    match key.code {
        KeyCode::Up if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(SqlCompletionMessage::MoveSelection { delta: -1 })
        }
        KeyCode::Down if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(SqlCompletionMessage::MoveSelection { delta: 1 })
        }
        // A bare Enter applies the highlighted completion; Alt+Enter (or any
        // modified Enter) is NOT consumed here so it falls through to the
        // editor's run-SQL accelerator.
        KeyCode::Enter if key.modifiers.is_empty() => Some(SqlCompletionMessage::Apply),
        // A bare Tab applies the highlighted completion too (matching the
        // original dbm's `handle_popup_key`) instead of inserting a tab.
        KeyCode::Tab if key.modifiers.is_empty() => Some(SqlCompletionMessage::Apply),
        KeyCode::Esc => Some(SqlCompletionMessage::Close),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::msg::SqlCompletionMessage;
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn arrows_move_selection_but_jk_fall_through() {
        // Only the arrow keys move the selection; `j`/`k` are NOT bound so they
        // can be typed into the editor buffer.
        assert_eq!(
            key_to_msg(key(KeyCode::Up)),
            Some(SqlCompletionMessage::MoveSelection { delta: -1 })
        );
        assert_eq!(
            key_to_msg(key(KeyCode::Down)),
            Some(SqlCompletionMessage::MoveSelection { delta: 1 })
        );
        assert_eq!(key_to_msg(key(KeyCode::Char('j'))), None);
        assert_eq!(key_to_msg(key(KeyCode::Char('k'))), None);
    }

    #[test]
    fn enter_tab_esc_apply_or_close() {
        assert_eq!(
            key_to_msg(key(KeyCode::Enter)),
            Some(SqlCompletionMessage::Apply)
        );
        assert_eq!(
            key_to_msg(key(KeyCode::Tab)),
            Some(SqlCompletionMessage::Apply)
        );
        assert_eq!(
            key_to_msg(key(KeyCode::Esc)),
            Some(SqlCompletionMessage::Close)
        );
        // Modified Enter is not consumed (falls through to the run-SQL key).
        let alt_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
        assert_eq!(key_to_msg(alt_enter), None);
    }
}
