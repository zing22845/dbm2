//! Editor feature intents.

use crate::app_shell::intent::Intent;
use super::msg::EditorMsg;
use super::context_picker::intent::ContextPickerIntent;
use super::sql_completion::intent::SqlCompletionIntent;

#[derive(Debug, Clone)]
pub enum EditorIntent {
    ContextPicker(ContextPickerIntent),
    SqlCompletion(SqlCompletionIntent),
}

impl Intent for EditorIntent {
    type Message = EditorMsg;

    fn into_message(self) -> Self::Message {
        match self {
            EditorIntent::ContextPicker(i) => i.into_message().into(),
            EditorIntent::SqlCompletion(i) => i.into_message().into(),
        }
    }
}

