//! Explorer objects feature intents.

use crate::app_shell::intent::Intent;
use super::msg::ObjectsMsg;

#[derive(Debug, Clone)]
pub enum ObjectsIntent {}

impl Intent for ObjectsIntent {
    type Message = ObjectsMsg;

    fn into_message(self) -> Self::Message {
        match self {}
    }
}
