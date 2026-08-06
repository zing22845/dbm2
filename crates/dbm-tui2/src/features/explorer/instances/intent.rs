//! Explorer instances feature intents.

use crate::app_shell::intent::Intent;
use super::msg::{InstancesMessage, InstancesMsg};

/// Intents emitted by the instances (connection tree) feature.
///
/// These are cross-feature interactions: selecting an instance opens the
/// instance workspace, and selecting a connection opens the SQL workspace. The
/// receiving features consume these at the shell/aggregation layer.
#[derive(Debug, Clone)]
pub enum InstancesIntent {
    /// The user activated an instance row: open the instance workspace for it.
    OpenInstanceWorkspace { instance_idx: usize },
    /// The user activated a connection row: open the SQL workspace for it.
    OpenConnectionWorkspace {
        instance_idx: usize,
        connection_idx: usize,
    },
}

impl Intent for InstancesIntent {
    type Message = InstancesMsg;

    fn into_message(self) -> Self::Message {
        // Cross-feature intents have no instances-local feedback; the shell
        // (`app/update.rs`) intercepts them and opens the target workspace, so
        // this mapping only satisfies the `Intent` trait and is never routed.
        match self {
            InstancesIntent::OpenInstanceWorkspace { .. }
            | InstancesIntent::OpenConnectionWorkspace { .. } => {
                InstancesMsg::Message(InstancesMessage::MoveUp)
            }
        }
    }
}
