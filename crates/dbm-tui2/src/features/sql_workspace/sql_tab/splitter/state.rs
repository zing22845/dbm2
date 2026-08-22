//! State owned by the `sql_tab` splitter feature: the widths/ratios set by the
//! tab's two resizable splitters.

/// Minimum width of the SQL editor pane, so a wide History zone can never push
/// the editor to zero (mirrors the original dbm's `MIN_SQL_PANE_WIDTH`).
pub const MIN_SQL_PANE_WIDTH: u16 = 20;

/// History pane width range (the editor/history splitter A clamps to this).
pub const MIN_HISTORY_WIDTH: u16 = 16;
pub const MAX_HISTORY_WIDTH: u16 = 200;
/// Default History pane width (matches the original dbm's default).
pub const DEFAULT_HISTORY_WIDTH: u16 = 24;

/// State of the SQL tab's two top-level splitters: the editor/history vertical
/// splitter (A) and the editor+history/results horizontal splitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqlTabSplitterState {
    /// Width of the History pane (the right side of the editor/history split).
    pub history_pane_width: u16,
    /// Editor top-pane height as a percentage of the body height (the
    /// editor+history row vs results split).
    pub split_ratio: u8,
}

impl Default for SqlTabSplitterState {
    fn default() -> Self {
        Self {
            history_pane_width: DEFAULT_HISTORY_WIDTH,
            split_ratio: 45,
        }
    }
}

impl SqlTabSplitterState {
    /// Set and clamp the History pane width (drag splitter A).
    pub fn set_history_pane_width(&mut self, width: u16) {
        self.history_pane_width = width.clamp(MIN_HISTORY_WIDTH, MAX_HISTORY_WIDTH);
    }

    /// Set and clamp the editor top-pane ratio (drag the editor/results split).
    /// The ratio is a percentage of the body height, clamped to [20, 80] so
    /// neither the top row nor results collapses.
    pub fn set_split_ratio(&mut self, ratio: u8) {
        self.split_ratio = ratio.clamp(20, 80);
    }
}
