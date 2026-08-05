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

    fn into_message(self) -> Self::Message {
        match self {
            HistoryIntent::Recall { .. } => unreachable!(
                "HistoryIntent::Recall is routed by sql_tab, not re-dispatched"
            ),
        }
    }
}
