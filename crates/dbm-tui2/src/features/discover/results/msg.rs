//! Discovery results feature messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsMessage {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsMsg {
    Message(ResultsMessage),
}

impl From<ResultsMessage> for ResultsMsg {
    fn from(m: ResultsMessage) -> Self {
        ResultsMsg::Message(m)
    }
}
