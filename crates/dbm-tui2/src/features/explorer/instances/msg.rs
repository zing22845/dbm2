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
    /// Expand/collapse the instance at the given visible row (mouse marker
    /// click) without moving the cursor.
    ToggleExpandAt { row: usize },
    /// Scroll the tree horizontally by `delta` columns (`Left`/`Right`),
    /// matching the original dbm's tree horizontal scroll. `term_width` is the
    /// current terminal width in columns (used to approximate the explorer
    /// viewport so scrolling stops at the content boundary). No-op
    /// (dirty=false) when already at a scroll boundary.
    ScrollHorizontal { delta: i16, term_width: u16 },
    /// Activate the current row (instance -> instance workspace, connection ->
    /// SQL workspace, focusing an existing tab when present).
    Select,
    /// On a connection row, always open a fresh SQL editor (mirrors the
    /// original dbm's `n` key, which forces a new tab regardless of any
    /// already-open tabs for that connection).
    NewConnectionTab,
    /// Reload a specific instance's connections from the store (used after a
    /// connection is added/edited/deleted inside the instance workspace, so the
    /// tree reflects the change immediately).
    RefreshConnections { instance_idx: usize },
    /// Move the cursor to a specific visible row (mouse click).
    JumpTo { row: usize },
    /// Programmatically set the viewport start (scrollbar drag / wheel).
    /// Clamped to `[0, visible_count - viewport]`. Sets `scroll_locked = true`
    /// so the discover-style anchor does not fight the manual scroll until
    /// the next cursor move.
    SetVScroll { position: usize },
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
