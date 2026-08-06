//! Discovery results feature intents.

use crate::app_shell::intent::Intent;
use super::msg::ResultsMsg;

#[derive(Debug, Clone)]
pub enum ResultsIntent {}

impl Intent for ResultsIntent {
    type Message = ResultsMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {}
    }
}
