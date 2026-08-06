//! Explorer feature intents.

use crate::app_shell::intent::Intent;
use super::msg::ExplorerMsg;
use super::instances::intent::InstancesIntent;
use super::objects::intent::ObjectsIntent;

/// Intents emitted by the explorer feature. Child intents are wrapped so they
/// can be lifted into the global router via `ExplorerMsg`.
#[derive(Debug, Clone)]
pub enum ExplorerIntent {
    /// An intent originating from the instances list.
    Instances(InstancesIntent),
    /// An intent originating from the objects tree.
    Objects(ObjectsIntent),
}

impl Intent for ExplorerIntent {
    type Message = ExplorerMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {
            ExplorerIntent::Instances(i) => i.into_message().map(Into::into),
            ExplorerIntent::Objects(i) => i.into_message().map(Into::into),
        }
    }
}
