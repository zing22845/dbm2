//! Discovery results feature intents.

use super::msg::ResultsMsg;
use crate::app_shell::intent::Intent;

#[derive(Debug, Clone)]
pub enum ResultsIntent {}

impl Intent for ResultsIntent {
    type Message = ResultsMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {}
    }
}
