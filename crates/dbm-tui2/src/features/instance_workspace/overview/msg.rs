//! Instance overview feature messages.

/// The actual overview panel messages.
#[derive(Debug, Clone)]
pub enum OverviewMessage {
    /// Load the instance overview for `instance_name`.
    Load { instance_name: String },
    /// Re-load the current instance's overview.
    Reload,
    /// The store returned the instance. Boxed to keep the enum small (a
    /// `ManagedInstance` is large and would otherwise bloat every message).
    Loaded { instance: Box<dbm_store::ManagedInstance> },
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone)]
pub enum OverviewMsg {
    Message(OverviewMessage),
}

impl From<OverviewMessage> for OverviewMsg {
    fn from(m: OverviewMessage) -> Self {
        OverviewMsg::Message(m)
    }
}
