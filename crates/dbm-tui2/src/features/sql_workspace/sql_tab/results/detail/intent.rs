//! Results detail sub-module intents.

use super::msg::DetailMsg;
use crate::app_shell::intent::Intent;

#[derive(Debug, Clone)]
pub enum DetailIntent {}

impl Intent for DetailIntent {
    type Message = DetailMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {}
    }
}
