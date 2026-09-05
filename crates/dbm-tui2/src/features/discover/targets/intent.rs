//! Discovery targets editor feature intents.

use super::msg::TargetsMsg;
use crate::app_shell::intent::Intent;

#[derive(Debug, Clone)]
pub enum TargetsIntent {}

impl Intent for TargetsIntent {
    type Message = TargetsMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {}
    }
}
