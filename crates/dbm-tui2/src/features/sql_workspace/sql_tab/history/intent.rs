//! History feature intents.

use crate::app_shell::intent::Intent;
use super::msg::{HistoryMessage, HistoryMsg};

/// Intents emitted by the history feature. `Recall` is resolved by `sql_tab`
/// (which owns the editor) by replacing the editor's SQL text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryIntent {
    /// Recall `sql` into the tab's editor.
    Recall { sql: String },
    /// Record a successfully executed statement for the connection. Emitted
    /// when a query result lands (`SetResult`), mirroring the original dbm's
    /// "record on success" behavior.
    RecordSuccess { instance: String, connection: String, sql: String },
}

impl Intent for HistoryIntent {
    type Message = HistoryMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {
            // Cross-feature: `Recall` is routed by `sql_tab` (it owns the
            // editor); it has no history sub-module message.
            HistoryIntent::Recall { .. } => None,
            // `RecordSuccess` is a plain sub-module message.
            HistoryIntent::RecordSuccess { instance, connection, sql } => Some(HistoryMsg::Message(
                HistoryMessage::RecordSuccess { instance, connection, sql },
            )),
        }
    }
}
