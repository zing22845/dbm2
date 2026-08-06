//! History feature intents.

use crate::app_shell::intent::Intent;
use super::msg::HistoryMsg;

/// Intents emitted by the history feature. `Recall` is resolved by `sql_tab`
/// (which owns the editor) by replacing the editor's SQL text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryIntent {
    /// Recall `sql` into the tab's editor.
    Recall { sql: String },
}

impl Intent for HistoryIntent {
    type Message = HistoryMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {
            // Cross-feature: `Recall` is routed by `sql_tab` (it owns the
            // editor); it has no history sub-module message.
            HistoryIntent::Recall { .. } => None,
        }
    }
}
