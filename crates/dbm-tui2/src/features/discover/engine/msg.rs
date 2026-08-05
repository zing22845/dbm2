//! Engine selector feature messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineMessage {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineMsg {
    Message(EngineMessage),
}

impl From<EngineMessage> for EngineMsg {
    fn from(m: EngineMessage) -> Self {
        EngineMsg::Message(m)
    }
}
