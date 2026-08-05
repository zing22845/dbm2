//! Instance overview feature intents.

use crate::app_shell::intent::Intent;
use super::msg::OverviewMsg;

#[derive(Debug, Clone)]
pub enum OverviewIntent {}

impl Intent for OverviewIntent {
    type Message = OverviewMsg;

    fn into_message(self) -> Self::Message {
        match self {}
    }
}
