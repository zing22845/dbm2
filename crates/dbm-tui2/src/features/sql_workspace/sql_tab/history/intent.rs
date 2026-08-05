//! History feature intents.

use crate::app_shell::intent::Intent;
use super::msg::HistoryMsg;

#[derive(Debug, Clone)]
pub enum HistoryIntent {}

impl Intent for HistoryIntent {
    type Message = HistoryMsg;

    fn into_message(self) -> Self::Message {
        match self {}
    }
}

