//! Instance workspace feature state.

use super::connections::state::ConnectionsState;
use super::overview::state::OverviewState;

/// State for the instance workspace feature, aggregating its two child
/// sub-modules.
#[derive(Debug, Default, Clone)]
pub struct IwState {
    /// The instance overview panel.
    pub overview: OverviewState,
    /// The instance connections panel.
    pub connections: ConnectionsState,
}
