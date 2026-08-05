//! Explorer objects feature messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectsMessage {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectsMsg {
    Message(ObjectsMessage),
}

impl From<ObjectsMessage> for ObjectsMsg {
    fn from(m: ObjectsMessage) -> Self {
        ObjectsMsg::Message(m)
    }
}
