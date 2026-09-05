//! Instance connections feature intents.

use super::msg::ConnectionsMsg;
use crate::app_shell::intent::Intent;

/// Intents emitted by the connections panel.
#[derive(Debug, Clone)]
pub enum ConnectionsIntent {
    /// A connection was added/edited/deleted; the explorer should refresh the
    /// tree for `instance_name` so the change shows up on the left side.
    ConnectionsChanged { instance_name: String },
}

impl Intent for ConnectionsIntent {
    type Message = ConnectionsMsg;

    fn into_message(self) -> Option<Self::Message> {
        // One-way notification to the shell (refresh the explorer tree); it is
        // consumed at the shell layer, so it declines a message here.
        match self {
            ConnectionsIntent::ConnectionsChanged { .. } => None,
        }
    }
}
