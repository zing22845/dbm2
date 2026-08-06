//! Editor feature intents.

use crate::app_shell::intent::Intent;
use super::msg::EditorMsg;
use super::context_picker::intent::ContextPickerIntent;
use super::sql_completion::intent::SqlCompletionIntent;

#[derive(Debug, Clone)]
pub enum EditorIntent {
    ContextPicker(ContextPickerIntent),
    SqlCompletion(SqlCompletionIntent),
    /// Run the editor's current SQL (resolved by `sql_tab`, which knows the
    /// tab's connection context).
    RunQuery { sql: String },
}

impl Intent for EditorIntent {
    type Message = EditorMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {
            EditorIntent::ContextPicker(i) => i.into_message().map(Into::into),
            EditorIntent::SqlCompletion(i) => i.into_message().map(Into::into),
            // Cross-feature: `RunQuery` is routed by `sql_tab` (which owns the
            // connection context); it has no editor sub-module message.
            EditorIntent::RunQuery { .. } => None,
        }
    }
}

