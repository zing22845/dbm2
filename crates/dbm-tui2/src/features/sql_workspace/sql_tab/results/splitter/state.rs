//! State owned by the Results-internal detail/list splitter.

/// Width of the splitter strip (single column, matches the drawn "│" bar).
pub const SPLITTER_WIDTH: u16 = 1;
/// Minimum allowed width of the detail pane.
pub const MIN_DETAIL_PANE_WIDTH: u16 = 24;
/// Maximum allowed width of the detail pane.
pub const MAX_DETAIL_PANE_WIDTH: u16 = 72;
/// Default detail pane width (matches the original dbm).
pub const DEFAULT_DETAIL_PANE_WIDTH: u16 = 40;

/// Clamp a detail pane width to the allowed range.
pub fn clamp_detail_pane_width(w: u16) -> u16 {
    w.clamp(MIN_DETAIL_PANE_WIDTH, MAX_DETAIL_PANE_WIDTH)
}

/// State of the Results-internal detail/list splitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitterState {
    /// Width of the detail preview pane (right side of the splitter).
    pub detail_pane_width: u16,
}

impl Default for SplitterState {
    fn default() -> Self {
        Self {
            detail_pane_width: DEFAULT_DETAIL_PANE_WIDTH,
        }
    }
}

impl SplitterState {
    /// Set and clamp the detail pane width.
    pub fn set_detail_pane_width(&mut self, width: u16) {
        self.detail_pane_width = clamp_detail_pane_width(width);
    }
}
