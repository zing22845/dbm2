//! Instance connections feature messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionsMessage {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionsMsg {
    Message(ConnectionsMessage),
}

impl From<ConnectionsMessage> for ConnectionsMsg {
    fn from(m: ConnectionsMessage) -> Self {
        ConnectionsMsg::Message(m)
    }
}
