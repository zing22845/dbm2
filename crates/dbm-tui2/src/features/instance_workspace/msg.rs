//! Instance workspace feature messages.

use super::connections::msg::ConnectionsMsg;
use super::overview::msg::OverviewMsg;

/// The actual instance workspace messages: forwarded to the two child
/// sub-modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IwMessage {
    /// Forwarded overview panel message.
    Overview(OverviewMsg),
    /// Forwarded connections panel message.
    Connections(ConnectionsMsg),
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IwMsg {
    Message(IwMessage),
}

impl From<IwMessage> for IwMsg {
    fn from(m: IwMessage) -> Self {
        IwMsg::Message(m)
    }
}

impl From<OverviewMsg> for IwMsg {
    fn from(m: OverviewMsg) -> Self {
        IwMsg::Message(IwMessage::Overview(m))
    }
}
impl From<ConnectionsMsg> for IwMsg {
    fn from(m: ConnectionsMsg) -> Self {
        IwMsg::Message(IwMessage::Connections(m))
    }
}
