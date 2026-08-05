//! Global footer feature state.

/// State for the global footer bar.
///
/// The footer is a global (always-handled) feature: it renders the fixed
/// shortcut hints plus an optional status line. The status lives here, on the
/// feature's own state, so `view::render` only depends on `FooterState` (see
/// the by-value, feature-self-contained convention used across this tree).
#[derive(Debug, Default, Clone)]
pub struct FooterState {
    /// Optional status text shown on its own line below the hints.
    pub status: String,
}
