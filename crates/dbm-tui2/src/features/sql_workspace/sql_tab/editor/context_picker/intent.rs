//! Context picker sub-module intents.

use super::msg::ContextPickerMsg;
use crate::app_shell::intent::Intent;

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

    fn into_message(self) -> Option<Self::Message> {
        // Cross-feature: `ApplyContext` is intercepted by `SqlTabIntent`
        // routing and resolved into a session update; it has no picker
        // sub-module message.
        match self {
            ContextPickerIntent::ApplyContext { .. } => None,
        }
    }
}
