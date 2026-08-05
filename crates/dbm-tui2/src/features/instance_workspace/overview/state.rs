//! Instance overview feature state.

use dbm_store::ManagedInstance;

/// State for the instance overview panel.
#[derive(Debug, Clone, Default)]
pub struct OverviewState {
    /// The instance being overviewed (loaded from the store).
    pub instance: Option<ManagedInstance>,
    /// The instance name (used to re-load on demand).
    pub instance_name: String,
}
