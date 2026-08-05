//! Explorer instances feature intents.

use crate::app_shell::intent::Intent;
use super::msg::InstancesMsg;

#[derive(Debug, Clone)]
pub enum InstancesIntent {}

impl Intent for InstancesIntent {
    type Message = InstancesMsg;

    fn into_message(self) -> Self::Message {
        match self {}
    }
}
