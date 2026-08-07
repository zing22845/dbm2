//! Discover feature state.
//!
//! Focus for the discover parent pane (engine / targets / results) lives on the
//! shell's `Pane::Discover(DiscoverPane)` — discover is a parent pane whose
//! child panes are the focused region while it is open. `DiscoverState` holds
//! only the feature's content state.

use super::engine::state::EngineState;
use super::results::state::ResultsState;
use super::targets::state::TargetsState;

/// State for the discover feature, aggregating its three child sub-modules.
#[derive(Debug, Clone, Default)]
pub struct DiscoverState {
    /// The discovery engine selector.
    pub engine: EngineState,
    /// The discovery targets editor.
    pub targets: TargetsState,
    /// The discovery results list.
    pub results: ResultsState,
    /// Whether the close-confirmation dialog is shown.
    pub close_confirm: bool,
    /// Whether a scan is currently in flight.
    pub scanning: bool,
    /// The last scan error, if any (cleared on a successful scan or new scan).
    pub last_error: Option<String>,
    /// The result of the last register attempt (success count or failure
    /// reason), shown in the discover zone footer. Cleared on reopen.
    pub register_message: Option<String>,
}

impl DiscoverState {
    /// Open a fresh discover modal with the default loopback target.
    pub fn opened() -> Self {
        DiscoverState {
            targets: TargetsState::with_default_targets(),
            ..DiscoverState::default()
        }
    }
}
