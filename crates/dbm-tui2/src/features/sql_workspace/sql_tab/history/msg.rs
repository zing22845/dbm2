//! History feature messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryMessage {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryMsg {
    Message(HistoryMessage),
}

impl From<HistoryMessage> for HistoryMsg {
    fn from(m: HistoryMessage) -> Self {
        HistoryMsg::Message(m)
    }
}
