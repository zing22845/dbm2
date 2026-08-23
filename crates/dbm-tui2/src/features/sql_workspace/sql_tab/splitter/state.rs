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

/// Default editor top-pane height in rows (the editor+history row vs results
/// split). Chosen to land near the old 45% default on a typical ~40-row body;
/// the layout clamps it to `[20%, 80%]` of the current track anyway.
pub const DEFAULT_EDITOR_TOP_HEIGHT: u16 = 18;

/// State of the SQL tab's two top-level splitters: the editor/history vertical
/// splitter (A) and the editor+history/results horizontal splitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqlTabSplitterState {
    /// Width of the History pane (the right side of the editor/history split).
    pub history_pane_width: u16,
    /// Editor top-pane **height in rows** (the editor+history row vs results
    /// split). Stored in absolute rows so dragging the splitter records the
    /// exact row under the pointer (no percent round-trip loss); it is clamped
    /// to `[20%, 80%]` of the current track at layout time. The percentage is
    /// only materialized for session persistence / terminal resizes.
    pub editor_top_height: u16,
}

impl Default for SqlTabSplitterState {
    fn default() -> Self {
        Self {
            history_pane_width: DEFAULT_HISTORY_WIDTH,
            editor_top_height: DEFAULT_EDITOR_TOP_HEIGHT,
        }
    }
}

impl SqlTabSplitterState {
    /// Set and clamp the History pane width (drag splitter A).
    pub fn set_history_pane_width(&mut self, width: u16) {
        self.history_pane_width = width.clamp(MIN_HISTORY_WIDTH, MAX_HISTORY_WIDTH);
    }

    /// Set and clamp the editor top-pane height in rows (drag the
    /// editor/results split). A generous floor/ceiling bounds the stored rows;
    /// the layout re-clamps them to `[20%, 80%]` of the current track.
    pub fn set_editor_top_height(&mut self, height: u16) {
        self.editor_top_height = height.clamp(1, 1000);
    }

    /// The editor top-pane height as a percentage of the given track height
    /// (rows), used only for session persistence / terminal resizes.
    pub fn editor_top_pct(&self, track_h: u16) -> u8 {
        if track_h == 0 {
            return 20;
        }
        let pct = (u32::from(self.editor_top_height) * 100) / u32::from(track_h);
        pct.clamp(20, 80) as u8
    }

    /// Set the editor top-pane height from a percentage of the given track
    /// height (rows), used when restoring a persisted session whose stored
    /// split is a percentage.
    pub fn set_editor_top_pct(&mut self, pct: u8, track_h: u16) {
        let rows = (u32::from(track_h) * u32::from(pct.clamp(20, 80))) / 100;
        self.editor_top_height = rows.clamp(1, 1000) as u16;
    }
}
