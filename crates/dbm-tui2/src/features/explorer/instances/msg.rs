//! Explorer instances feature messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstancesMessage {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstancesMsg {
    Message(InstancesMessage),
}

impl From<InstancesMessage> for InstancesMsg {
    fn from(m: InstancesMessage) -> Self {
        InstancesMsg::Message(m)
    }
}
