//! Explorer feature messages.

use super::instances::msg::InstancesMsg;
use super::objects::msg::ObjectsMsg;

/// The actual explorer messages: forwarded to the two child sub-modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerMessage {
    /// Forwarded instances list message.
    Instances(InstancesMsg),
    /// Forwarded objects tree message.
    Objects(ObjectsMsg),
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerMsg {
    Message(ExplorerMessage),
}

impl From<ExplorerMessage> for ExplorerMsg {
    fn from(m: ExplorerMessage) -> Self {
        ExplorerMsg::Message(m)
    }
}

impl From<InstancesMsg> for ExplorerMsg {
    fn from(m: InstancesMsg) -> Self {
        ExplorerMsg::Message(ExplorerMessage::Instances(m))
    }
}
impl From<ObjectsMsg> for ExplorerMsg {
    fn from(m: ObjectsMsg) -> Self {
        ExplorerMsg::Message(ExplorerMessage::Objects(m))
    }
}
