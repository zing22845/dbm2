//! Results detail sub-module messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailMessage {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailMsg {
    Message(DetailMessage),
}

impl From<DetailMessage> for DetailMsg {
    fn from(m: DetailMessage) -> Self {
        DetailMsg::Message(m)
    }
}
