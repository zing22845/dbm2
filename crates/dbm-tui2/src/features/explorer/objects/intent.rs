//! Explorer objects feature intents.

use super::msg::ObjectsMsg;
use super::state::ObjectsTarget;
use crate::app_shell::intent::Intent;

/// Intents emitted by the objects tree.
#[derive(Debug, Clone)]
pub enum ObjectsIntent {
    /// The user activated an object; open it in the SQL workspace.
    OpenObject { target: ObjectsTarget },
    /// The user activated a schema row; apply it as the active database/schema
    /// of the bound SQL tab (mirroring the original dbm's `apply_objects_schema`).
    ApplySchema { database: String, name: String },
}

impl Intent for ObjectsIntent {
    type Message = ObjectsMsg;

    fn into_message(self) -> Option<Self::Message> {
        // Cross-feature one-way notifications to the shell; the shell
        // (`app/update.rs`) intercepts them, so they decline a message and the
        // router skips them.
        match self {
            ObjectsIntent::OpenObject { .. } | ObjectsIntent::ApplySchema { .. } => None,
        }
    }
}
