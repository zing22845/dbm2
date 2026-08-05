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

    fn into_message(self) -> Self::Message {
        match self {
            EditorIntent::ContextPicker(i) => i.into_message().into(),
            EditorIntent::SqlCompletion(i) => i.into_message().into(),
            EditorIntent::RunQuery { .. } => unreachable!(
                "EditorIntent::RunQuery is routed by sql_tab, not re-dispatched"
            ),
        }
    }
}

