//! Instance connections feature intents.

use crate::app_shell::intent::Intent;
use super::msg::{ConnectionsMessage, ConnectionsMsg};

/// Intents emitted by the connections panel.
#[derive(Debug, Clone)]
pub enum ConnectionsIntent {
    /// A connection was added/edited/deleted; the explorer should refresh its
    /// tree.
    ConnectionsChanged,
}

impl Intent for ConnectionsIntent {
    type Message = ConnectionsMsg;

    fn into_message(self) -> Self::Message {
        // One-way notification to the shell (refresh the explorer tree); the
        // message mapping is total but currently unused.
        match self {
            ConnectionsIntent::ConnectionsChanged => {
                ConnectionsMsg::Message(ConnectionsMessage::MoveUp)
            }
        }
    }
}
