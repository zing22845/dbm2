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

    // Skeleton state: the child feature messages are currently uninhabited
    // because their leaf messages are empty enums. Remove this allow when real
    // business messages are introduced.
    #[allow(unreachable_code)]
    fn into_message(self) -> Self::Message {
        match self {
            IwIntent::Overview(i) => i.into_message().into(),
            IwIntent::Connections(i) => i.into_message().into(),
        }
    }
}
