//! Global footer feature intents.

use crate::app_shell::intent::Intent;
use super::msg::FooterMsg;

#[derive(Debug, Clone)]
pub enum FooterIntent {}

impl Intent for FooterIntent {
    type Message = FooterMsg;

    fn into_message(self) -> Self::Message {
        match self {}
    }
}
