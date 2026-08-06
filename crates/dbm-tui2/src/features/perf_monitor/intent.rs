//! Performance monitor feature intents.

use crate::app_shell::intent::Intent;
use super::msg::PerfMsg;

#[derive(Debug, Clone)]
pub enum PerfIntent {}

impl Intent for PerfIntent {
    type Message = PerfMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {}
    }
}
