//! State owned by the History splitter feature (B): the detail pane width.

/// Width range of the History detail preview pane (splitter B clamps to this).
pub const MIN_DETAIL_PANE_WIDTH: u16 = 24;
pub const MAX_DETAIL_PANE_WIDTH: u16 = 72;
/// Default detail pane width (matches the original dbm's default).
pub const DEFAULT_DETAIL_PANE_WIDTH: u16 = 40;

/// Clamp a detail pane width to the allowed range.
pub fn clamp_detail_pane_width(w: u16) -> u16 {
    w.clamp(MIN_DETAIL_PANE_WIDTH, MAX_DETAIL_PANE_WIDTH)
}

/// State of the History-internal detail/list splitter (B).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetailSplitterState {
    /// Width of the detail preview pane (left side of the splitter B).
    pub detail_pane_width: u16,
}

impl Default for DetailSplitterState {
    fn default() -> Self {
        Self {
            detail_pane_width: DEFAULT_DETAIL_PANE_WIDTH,
        }
    }
}

impl DetailSplitterState {
    /// Set and clamp the detail pane width (drag splitter B).
    pub fn set_detail_pane_width(&mut self, width: u16) {
        self.detail_pane_width = clamp_detail_pane_width(width);
    }
}
