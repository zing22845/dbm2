//! Engine selector feature intents.

use crate::app_shell::intent::Intent;
use super::msg::EngineMsg;

#[derive(Debug, Clone)]
pub enum EngineIntent {}

impl Intent for EngineIntent {
    type Message = EngineMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {}
    }
}
