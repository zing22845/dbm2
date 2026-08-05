//! Context picker sub-module intents.

use crate::app_shell::intent::Intent;
use super::msg::ContextPickerMsg;

/// Intents emitted by the context picker. Only one exists today: applying a
/// selection, which the `sql_tab` parent resolves into a session update (the
/// picker is a child of the editor and cannot touch the sibling session).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextPickerIntent {
    /// The user selected a database/schema; apply it to the tab's session.
    ApplyContext { database: String, schema: String },
}

impl Intent for ContextPickerIntent {
    type Message = ContextPickerMsg;

    fn into_message(self) -> Self::Message {
        // The apply intent is not a message the picker handles itself; it is
        // intercepted by `SqlTabIntent` routing and resolved into a session
        // update. This arm is unreachable for a correctly routed intent.
        match self {
            ContextPickerIntent::ApplyContext { .. } => unreachable!(
                "ContextPickerIntent::ApplyContext is routed by sql_tab, not re-dispatched"
            ),
        }
    }
}
