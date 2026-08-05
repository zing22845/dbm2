//! Performance monitor feature messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PerfMessage {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PerfMsg {
    Message(PerfMessage),
}

impl From<PerfMessage> for PerfMsg {
    fn from(m: PerfMessage) -> Self {
        PerfMsg::Message(m)
    }
}
