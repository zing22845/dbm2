//! Instance overview feature state.

use dbm_store::ManagedInstance;

/// State for the instance overview panel.
#[derive(Debug, Clone, Default)]
pub struct OverviewState {
    /// The instance being overviewed (loaded from the store).
    pub instance: Option<ManagedInstance>,
    /// The instance name (used to re-load on demand).
    pub instance_name: String,
    /// Cursor row within the overview list, highlighted like the original dbm.
    pub cursor: usize,
    /// Horizontal scroll offset (columns) for long rows, like the original dbm.
    pub h_scroll: u16,
}
