//! Results list sub-module intents.

use crate::app_shell::intent::Intent;
use super::msg::ListMsg;

#[derive(Debug, Clone)]
pub enum ListIntent {}

impl Intent for ListIntent {
    type Message = ListMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {}
    }
}