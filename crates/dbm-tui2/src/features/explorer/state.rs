//! Explorer feature state.

use super::instances::state::InstancesState;
use super::objects::state::ObjectsState;

// Re-export the shared explorer sub-pane type so the shell's `Pane` and the
// explorer feature agree on the same navigation type.
pub use crate::app_shell::nav::ExplorerPane;

/// State for the explorer feature, aggregating its two child sub-modules.
#[derive(Debug, Clone, Default)]
pub struct ExplorerState {
    /// The instances / connection tree.
    pub instances: InstancesState,
    /// The object tree.
    pub objects: ObjectsState,
    /// Which explorer pane is focused.
    pub pane: ExplorerPane,
}
