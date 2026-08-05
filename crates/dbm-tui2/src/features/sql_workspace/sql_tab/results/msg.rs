//! Results feature messages.

use super::detail::msg::DetailMsg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsMessage {
    Detail(DetailMsg),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsMsg {
    Message(ResultsMessage),
}

impl From<ResultsMessage> for ResultsMsg {
    fn from(m: ResultsMessage) -> Self {
        ResultsMsg::Message(m)
    }
}

impl From<DetailMsg> for ResultsMsg {
    fn from(m: DetailMsg) -> Self {
        ResultsMsg::Message(ResultsMessage::Detail(m))
    }
}
