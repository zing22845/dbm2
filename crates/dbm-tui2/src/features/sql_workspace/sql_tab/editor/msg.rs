//! Editor feature messages.

use crossterm::event::KeyEvent;

use super::context_picker::msg::ContextPickerMsg;
use super::sql_completion::msg::SqlCompletionMsg;

/// The actual editor messages: buffer edits plus forwarding to sub-modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorMessage {
    /// Forward a normalized key to the editor buffer.
    KeyEvent {
        key: KeyEvent,
        tracked_caps_lock: bool,
    },
    /// Paste `text` at the cursor.
    Paste { text: String },
    /// Replace the whole buffer with `sql`.
    SetSql { sql: String },
    /// Forward to the context picker sub-module.
    ContextPicker(ContextPickerMsg),
    /// Forward to the SQL completion sub-module.
    SqlCompletion(SqlCompletionMsg),
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorMsg {
    Message(EditorMessage),
}

impl From<EditorMessage> for EditorMsg {
    fn from(m: EditorMessage) -> Self {
        EditorMsg::Message(m)
    }
}

impl From<ContextPickerMsg> for EditorMsg {
    fn from(m: ContextPickerMsg) -> Self {
        EditorMsg::Message(EditorMessage::ContextPicker(m))
    }
}
impl From<SqlCompletionMsg> for EditorMsg {
    fn from(m: SqlCompletionMsg) -> Self {
        EditorMsg::Message(EditorMessage::SqlCompletion(m))
    }
}
