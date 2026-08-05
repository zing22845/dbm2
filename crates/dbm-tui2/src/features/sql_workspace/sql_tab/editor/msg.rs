//! Editor feature messages.

use super::context_picker::msg::ContextPickerMsg;
use super::sql_completion::msg::SqlCompletionMsg;

/// The actual editor messages. Skeleton: forwards to sub-modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorMessage {
    ContextPicker(ContextPickerMsg),
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
