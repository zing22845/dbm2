//! Discovery targets editor feature messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetsMessage {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetsMsg {
    Message(TargetsMessage),
}

impl From<TargetsMessage> for TargetsMsg {
    fn from(m: TargetsMessage) -> Self {
        TargetsMsg::Message(m)
    }
}
