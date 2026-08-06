//! Explorer objects feature intents.

use crate::app_shell::intent::Intent;
use super::msg::ObjectsMsg;
use super::state::ObjectsTarget;

/// Intents emitted by the objects tree.
#[derive(Debug, Clone)]
pub enum ObjectsIntent {
    /// The user activated an object; open it in the SQL workspace.
    OpenObject { target: ObjectsTarget },
}

impl Intent for ObjectsIntent {
    type Message = ObjectsMsg;

    fn into_message(self) -> Option<Self::Message> {
        // Cross-feature one-way notification to the shell; the shell
        // (`app/update.rs`) intercepts `OpenObject` and opens a SQL tab, so it
        // declines a message and the router skips it.
        match self {
            ObjectsIntent::OpenObject { .. } => None,
        }
    }
}
