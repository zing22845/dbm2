//! Instance connections feature intents.

use crate::app_shell::intent::Intent;
use super::msg::ConnectionsMsg;

#[derive(Debug, Clone)]
pub enum ConnectionsIntent {}

impl Intent for ConnectionsIntent {
    type Message = ConnectionsMsg;

    fn into_message(self) -> Self::Message {
        match self {}
    }
}
