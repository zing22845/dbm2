//! SQL completion sub-module intents.

use crate::app_shell::intent::Intent;
use super::msg::SqlCompletionMsg;

#[derive(Debug, Clone)]
pub enum SqlCompletionIntent {}

impl Intent for SqlCompletionIntent {
    type Message = SqlCompletionMsg;

    fn into_message(self) -> Self::Message {
        match self {}
    }
}

