//! Instance overview feature state.

use std::time::Instant;

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
    /// Vertical scroll offset in data rows (0 = top row visible).
    pub scroll: usize,
    /// When true, the discover-style cursor anchor is skipped — the viewport
    /// stays exactly where the user last dragged/scrolled it, even if the
    /// cursor moves outside it. Reset on any MoveCursor.
    pub scroll_locked: bool,
    /// One-line status shown on the overview pane footer (e.g. "Refreshed").
    /// Owned by this pane so it does not leak into the connections footer.
    pub status: Option<String>,
    /// Refresh cooldown deadline: the overview's `r` is ignored until this
    /// instant passes (matching the original dbm's 1s cooldown). Owned by this
    /// pane — the connections pane has no refresh action.
    pub refresh_cooldown_until: Option<Instant>,
}
