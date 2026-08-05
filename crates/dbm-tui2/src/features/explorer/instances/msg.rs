//! Explorer instances feature messages.

/// The actual instances (connection tree) messages.
#[derive(Debug, Clone)]
pub enum InstancesMessage {
    /// Load the instance tree from the store.
    Load,
    /// The store returned the loaded managed instances.
    Loaded { instances: Vec<dbm_store::ManagedInstance> },
    /// The store returned a specific instance's connections.
    ConnectionsLoaded { instance_idx: usize, connections: Vec<dbm_store::InstanceConnection> },
    /// Move the cursor up.
    MoveUp,
    /// Move the cursor down.
    MoveDown,
    /// Toggle expansion of the current instance.
    ToggleExpand,
    /// Activate the current row (instance -> instance workspace, connection ->
    /// SQL workspace).
    Select,
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone)]
pub enum InstancesMsg {
    Message(InstancesMessage),
}

impl From<InstancesMessage> for InstancesMsg {
    fn from(m: InstancesMessage) -> Self {
        InstancesMsg::Message(m)
    }
}
