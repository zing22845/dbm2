//! Instance overview feature messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverviewMessage {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverviewMsg {
    Message(OverviewMessage),
}

impl From<OverviewMessage> for OverviewMsg {
    fn from(m: OverviewMessage) -> Self {
        OverviewMsg::Message(m)
    }
}
