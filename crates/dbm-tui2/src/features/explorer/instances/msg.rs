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
    /// Expand the current instance (loading its connections), matching the
    /// original dbm's `l` key.
    Expand,
    /// Collapse the current instance, matching the original dbm's `h` key.
    Collapse,
    /// Scroll the tree horizontally by `delta` columns (`Left`/`Right`),
    /// matching the original dbm's tree horizontal scroll. `term_width` is the
    /// current terminal width in columns (used to approximate the explorer
    /// viewport so scrolling stops at the content boundary). No-op
    /// (dirty=false) when already at a scroll boundary.
    ScrollHorizontal { delta: i16, term_width: u16 },
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
