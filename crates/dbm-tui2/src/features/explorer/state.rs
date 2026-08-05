//! Explorer feature state.

use super::instances::state::InstancesState;
use super::objects::state::ObjectsState;

/// Which explorer pane is focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExplorerPane {
    /// The instances / connection tree.
    #[default]
    Instances,
    /// The object tree (migrated in a later phase).
    Objects,
}

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
