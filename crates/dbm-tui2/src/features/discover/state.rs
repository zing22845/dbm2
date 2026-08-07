//! Discover feature state.
//!
//! Focus for the discover parent pane (engine / targets / results) lives on the
//! shell's `Pane::Discover(DiscoverPane)` — discover is a parent pane whose
//! child panes are the focused region while it is open. `DiscoverState` holds
//! only the feature's content state.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

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
    /// Live scan progress (hosts done / hosts total), shown in the zone footer
    /// while scanning — mirrors the original dbm's `Scanning… hosts d/t`.
    pub scan_progress: Option<(u32, u32)>,
    /// Whether the user asked to cancel the in-flight scan (`c` key).
    pub cancelling: bool,
    /// The last scan was cancelled (shown as `cancelled` until a new scan).
    pub scan_cancelled: bool,
    /// Shared cancel flag handed to the scan effect so `CancelScan` can stop a
    /// running scan at the next host boundary.
    pub scan_cancel: Arc<AtomicBool>,
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
