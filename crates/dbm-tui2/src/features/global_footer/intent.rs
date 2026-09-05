//! Global footer feature intents.

use super::msg::FooterMsg;
use crate::app_shell::intent::Intent;

#[derive(Debug, Clone)]
pub enum FooterIntent {}

impl Intent for FooterIntent {
    type Message = FooterMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {}
    }
}
