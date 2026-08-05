//! Explorer feature messages.

use super::instances::msg::InstancesMsg;
use super::objects::msg::ObjectsMsg;
use super::state::ExplorerPane;

/// The actual explorer messages: pane navigation plus forwarding to the two
/// child sub-modules.
#[derive(Debug, Clone)]
pub enum ExplorerMessage {
    /// Set the active explorer pane.
    SetPane(ExplorerPane),
    /// Forwarded instances list message.
    Instances(InstancesMsg),
    /// Forwarded objects tree message.
    Objects(ObjectsMsg),
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone)]
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
