//! Results detail sub-module intents.

use crate::app_shell::intent::Intent;
use super::msg::DetailMsg;

#[derive(Debug, Clone)]
pub enum DetailIntent {}

impl Intent for DetailIntent {
    type Message = DetailMsg;

    fn into_message(self) -> Self::Message {
        match self {}
    }
}

