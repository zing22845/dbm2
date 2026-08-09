//! Instance workspace feature state.

use std::time::Instant;

use super::connections::state::ConnectionsState;
use super::overview::state::OverviewState;

/// State for the instance workspace feature, aggregating its two child
/// sub-modules.
#[derive(Debug, Clone, Default)]
pub struct IwState {
    /// The name of the instance currently open in the workspace.
    pub instance_name: String,
    /// Which instance-workspace sub-pane is active (overview / connections),
    /// rendered as a tab bar with the selected pane's body below it.
    pub pane: crate::app_shell::nav::IwPane,
    /// The instance overview panel.
    pub overview: OverviewState,
    /// The instance connections panel.
    pub connections: ConnectionsState,
    /// Refresh cooldown deadline: the overview's `r` is ignored until this
    /// instant passes (matching the original dbm's 1s cooldown).
    pub refresh_cooldown_until: Option<Instant>,
    /// One-line status shown on the pane footer (e.g. "Refreshed").
    pub status: Option<String>,
}
