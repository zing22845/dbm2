//! Instance overview feature intents.

use super::msg::OverviewMsg;
use crate::app_shell::intent::Intent;

#[derive(Debug, Clone)]
pub enum OverviewIntent {}

impl Intent for OverviewIntent {
    type Message = OverviewMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {}
    }
}
