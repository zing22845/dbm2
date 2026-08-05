//! Context picker sub-module intents.

use crate::app_shell::intent::Intent;
use super::msg::ContextPickerMsg;

#[derive(Debug, Clone)]
pub enum ContextPickerIntent {}

impl Intent for ContextPickerIntent {
    type Message = ContextPickerMsg;

    fn into_message(self) -> Self::Message {
        match self {}
    }
}

