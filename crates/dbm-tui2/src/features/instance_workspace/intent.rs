//! Instance workspace feature intents.

use crate::app_shell::intent::Intent;
use super::msg::IwMsg;
use super::connections::intent::ConnectionsIntent;
use super::overview::intent::OverviewIntent;

/// Intents emitted by the instance workspace feature. Child intents are
/// wrapped so they can be lifted into the global router via `IwMsg`.
#[derive(Debug, Clone)]
pub enum IwIntent {
    /// An intent originating from the overview panel.
    Overview(OverviewIntent),
    /// An intent originating from the connections panel.
    Connections(ConnectionsIntent),
}

impl Intent for IwIntent {
    type Message = IwMsg;

    fn into_message(self) -> Self::Message {
        match self {
            // Child intents route back to this feature; their message mappings
            // are placeholders until the receiving features are wired.
            IwIntent::Overview(i) => i.into_message().into(),
            IwIntent::Connections(i) => i.into_message().into(),
        }
    }
}
