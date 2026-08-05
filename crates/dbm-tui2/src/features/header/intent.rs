//! Header feature intents.

use crate::app_shell::intent::Intent;
use super::msg::HeaderMsg;

/// Intents emitted by the header feature. Empty in the skeleton.
#[derive(Debug, Clone)]
pub enum HeaderIntent {}

impl Intent for HeaderIntent {
    type Message = HeaderMsg;

    fn into_message(self) -> Self::Message {
        match self {}
    }
}
