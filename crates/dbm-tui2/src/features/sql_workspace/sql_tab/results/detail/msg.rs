//! Results detail sub-module messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailMessage {
    /// Scroll the detail body by `delta` display rows.
    Scroll { delta: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailMsg {
    Message(DetailMessage),
}

impl From<DetailMessage> for DetailMsg {
    fn from(m: DetailMessage) -> Self {
        DetailMsg::Message(m)
    }
}
