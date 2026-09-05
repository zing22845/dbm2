//! Results list sub-module intents.

use super::msg::ListMsg;
use crate::app_shell::intent::Intent;

#[derive(Debug, Clone)]
pub enum ListIntent {}

impl Intent for ListIntent {
    type Message = ListMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {}
    }
}
