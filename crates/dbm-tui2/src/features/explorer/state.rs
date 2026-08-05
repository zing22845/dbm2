//! Explorer feature state.

use super::instances::state::InstancesState;
use super::objects::state::ObjectsState;

/// State for the explorer feature, aggregating its two child sub-modules.
#[derive(Debug, Default, Clone)]
pub struct ExplorerState {
    /// The instances list sub-module.
    pub instances: InstancesState,
    /// The objects tree sub-module.
    pub objects: ObjectsState,
}
