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
    /// Request that the tab (re)load its SQL-completion catalog. Raised when
    /// the editor hits a table-intent slot (TblCmp ON) but has no cached table
    /// names yet, mirroring the original dbm's `schedule_metadata_refresh`.
    /// `sql_tab` resolves it into a `LoadCompletionCatalog` effect.
    LoadCompletionCatalog,
}

impl Intent for EditorIntent {
    type Message = EditorMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {
            EditorIntent::ContextPicker(i) => i.into_message().map(Into::into),
            EditorIntent::SqlCompletion(i) => i.into_message().map(Into::into),
            // Cross-feature: `RunQuery` and `LoadCompletionCatalog` are routed
            // by `sql_tab` (which owns the connection context); they have no
            // editor sub-module message.
            EditorIntent::RunQuery { .. } | EditorIntent::LoadCompletionCatalog => None,
        }
    }
}

