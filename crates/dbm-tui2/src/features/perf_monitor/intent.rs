//! Performance monitor feature intents.

use super::msg::PerfMsg;
use crate::app_shell::intent::Intent;

#[derive(Debug, Clone)]
pub enum PerfIntent {}

impl Intent for PerfIntent {
    type Message = PerfMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {}
    }
}
