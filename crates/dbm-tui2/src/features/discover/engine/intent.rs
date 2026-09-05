//! Engine selector feature intents.

use super::msg::EngineMsg;
use crate::app_shell::intent::Intent;

#[derive(Debug, Clone)]
pub enum EngineIntent {}

impl Intent for EngineIntent {
    type Message = EngineMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {}
    }
}
