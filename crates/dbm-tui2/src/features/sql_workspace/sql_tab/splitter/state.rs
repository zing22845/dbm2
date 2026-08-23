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
    /// the layout re-clamps them to `[20%, 80%]` of the current track. Returns
    /// `true` when the stored height actually changed, so callers can skip a
    /// redundant repaint (and keep the redundancy/waste metric low) when a drag
    /// does not move the splitter.
    pub fn set_editor_top_height(&mut self, height: u16) -> bool {
        let new = height.clamp(1, 1000);
        if new == self.editor_top_height {
            return false;
        }
        self.editor_top_height = new;
        true
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

    /// Nudge the editor top-pane height by one keyboard step. `plus` is the
    /// `+` / `-` key (true = `+`); `top_focused` is whether the focused sub-pane
    /// is the top row (editor/history) rather than the bottom (results). `+`
    /// always grows the focused pane and `-` shrinks it, so when the bottom is
    /// focused the top height moves opposite to the key. Returns `true` when the
    /// split actually moved.
    pub fn nudge_editor_top_height(&mut self, plus: bool, top_focused: bool) -> bool {
        use crate::common::view::splitter::WIDTH_NUDGE_STEP;
        // `+` grows the focused pane; the top height moves opposite to a
        // bottom focus.
        let grow_top = if plus { top_focused } else { !top_focused };
        let delta = if grow_top { WIDTH_NUDGE_STEP } else { -WIDTH_NUDGE_STEP };
        let next = (self.editor_top_height as i16 + delta).max(0) as u16;
        self.set_editor_top_height(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> SqlTabSplitterState {
        let mut s = SqlTabSplitterState::default();
        s.editor_top_height = 20;
        s
    }

    #[test]
    fn plus_grows_top_when_top_focused() {
        let mut s = state();
        assert!(s.nudge_editor_top_height(true, true));
        assert_eq!(s.editor_top_height, 22);
    }

    #[test]
    fn minus_shrinks_top_when_top_focused() {
        let mut s = state();
        assert!(s.nudge_editor_top_height(false, true));
        assert_eq!(s.editor_top_height, 18);
    }

    #[test]
    fn plus_shrinks_top_when_bottom_focused() {
        // `+` grows the focused (bottom) pane, so the top height drops.
        let mut s = state();
        assert!(s.nudge_editor_top_height(true, false));
        assert_eq!(s.editor_top_height, 18);
    }

    #[test]
    fn minus_grows_top_when_bottom_focused() {
        // `-` shrinks the focused (bottom) pane, so the top height grows.
        let mut s = state();
        assert!(s.nudge_editor_top_height(false, false));
        assert_eq!(s.editor_top_height, 22);
    }
}
