//! SQL completion sub-module intents.

use crate::app_shell::intent::Intent;
use crate::common::utils::cursor::Cursor;

use super::msg::SqlCompletionMsg;
use super::provider::CompletionItem;

/// Intents emitted by the SQL completion sub-module. The editor (parent)
/// resolves `Apply` by inserting the completion into the buffer.
#[derive(Debug, Clone)]
pub enum SqlCompletionIntent {
    /// Insert `item` into the editor, replacing `[replace_start, replace_end)`.
    Apply {
        item: CompletionItem,
        replace_start: Cursor,
        replace_end: Cursor,
    },
}

impl Intent for SqlCompletionIntent {
    type Message = SqlCompletionMsg;

    fn into_message(self) -> Option<Self::Message> {
        // Cross-feature: `Apply` is resolved by the editor (it inserts the
        // completion into the buffer); it has no completion sub-module message.
        match self {
            SqlCompletionIntent::Apply { .. } => None,
        }
    }
}
