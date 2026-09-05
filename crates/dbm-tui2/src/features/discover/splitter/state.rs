//! State of the discover targets/results horizontal splitter.

/// Default targets editor height in rows (the top pane). The layout clamps it
/// to `[20%, 80%]` of the current track anyway.
pub const DEFAULT_TARGETS_HEIGHT: u16 = 10;

/// State of the discover targets/results splitter. The targets editor height
/// is stored in **absolute rows** so dragging records the exact row under the
/// pointer (no percent round-trip loss); it is clamped to `[20%, 80%]` of the
/// current track at layout time. The percentage is only materialized for
/// session persistence / terminal resizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoverSplitterState {
    /// Targets editor (top pane) height in rows.
    pub targets_height: u16,
    /// The actual targets-height bounds (rows) the last layout clamped to,
    /// refreshed by the run loop from the layout; keyboard nudges clamp to these
    /// so the stored value and the rendered split never disagree.
    pub targets_min: u16,
    pub targets_max: u16,
}

impl Default for DiscoverSplitterState {
    fn default() -> Self {
        Self {
            targets_height: DEFAULT_TARGETS_HEIGHT,
            targets_min: 1,
            targets_max: 200,
        }
    }
}

impl DiscoverSplitterState {
    /// Set and clamp the targets height in rows (drag the splitter).
    pub fn set_targets_height(&mut self, height: u16) -> bool {
        let new = height.clamp(1, 1000);
        if new == self.targets_height {
            return false;
        }
        self.targets_height = new;
        true
    }

    /// The targets height as a percentage of the given track height (rows),
    /// used only for session persistence / terminal resizes.
    pub fn targets_height_pct(&self, track_h: u16) -> u8 {
        if track_h == 0 {
            return 20;
        }
        let pct = (u32::from(self.targets_height) * 100) / u32::from(track_h);
        pct.clamp(20, 80) as u8
    }

    /// Set the targets height from a percentage of the given track height
    /// (rows), used when restoring a persisted session whose stored split is a
    /// percentage.
    pub fn set_targets_height_pct(&mut self, pct: u8, track_h: u16) {
        let rows = (u32::from(track_h) * u32::from(pct.clamp(20, 80))) / 100;
        self.targets_height = rows.clamp(1, 1000) as u16;
    }

    /// Nudge the targets height by one keyboard step. `plus` is the `+` / `-`
    /// key (true = `+`); `top_focused` is whether the targets (top) pane is
    /// focused rather than the results (bottom). `+` always grows the focused
    /// pane and `-` shrinks it. Returns `true` when the split actually moved.
    pub fn nudge_targets_height(&mut self, plus: bool, top_focused: bool) -> bool {
        use crate::common::view::splitter::WIDTH_NUDGE_STEP;
        let grow_top = if plus { top_focused } else { !top_focused };
        let delta = if grow_top {
            WIDTH_NUDGE_STEP
        } else {
            -WIDTH_NUDGE_STEP
        };
        // Clamp to the layout's actual bounds intersected with the storage
        // range `set_targets_height` clamps to, so the target and stored value
        // always agree (no redundant repaint at the boundary).
        let lo = self.targets_min.max(1);
        let hi = self.targets_max.min(1000);
        let next = (self.targets_height as i16 + delta).clamp(lo as i16, hi as i16) as u16;
        self.set_targets_height(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_height_only_marks_dirty_on_change() {
        let mut s = DiscoverSplitterState::default();
        assert!(s.set_targets_height(15));
        assert_eq!(s.targets_height, 15);
        assert!(!s.set_targets_height(15), "unchanged height must not dirty");
        assert!(s.set_targets_height(16));
    }

    #[test]
    fn pct_round_trip() {
        let mut s = DiscoverSplitterState::default();
        s.set_targets_height_pct(50, 20);
        assert_eq!(s.targets_height, 10);
        assert_eq!(s.targets_height_pct(20), 50);
    }

    #[test]
    fn nudge_grows_focused_pane() {
        let mut s = DiscoverSplitterState::default();
        s.targets_height = 20;
        // + with targets focused grows the top (targets).
        assert!(s.nudge_targets_height(true, true));
        assert_eq!(s.targets_height, 22);
        // + with results focused shrinks the top (grows the bottom).
        let mut s2 = DiscoverSplitterState::default();
        s2.targets_height = 20;
        assert!(s2.nudge_targets_height(true, false));
        assert_eq!(s2.targets_height, 18);
        // - with targets focused shrinks the top.
        let mut s3 = DiscoverSplitterState::default();
        s3.targets_height = 20;
        assert!(s3.nudge_targets_height(false, true));
        assert_eq!(s3.targets_height, 18);
        // - with results focused grows the top.
        let mut s4 = DiscoverSplitterState::default();
        s4.targets_height = 20;
        assert!(s4.nudge_targets_height(false, false));
        assert_eq!(s4.targets_height, 22);
    }

    #[test]
    fn nudge_stops_dirtying_at_the_boundary() {
        let mut s = DiscoverSplitterState::default();
        s.targets_min = 2;
        s.targets_max = 7;
        s.targets_height = 3;
        assert!(s.nudge_targets_height(false, true)); // 3 -> 2
        assert_eq!(s.targets_height, 2);
        assert!(
            !s.nudge_targets_height(false, true),
            "below min must not dirty"
        );
        assert_eq!(s.targets_height, 2);
        s.targets_height = 6;
        assert!(s.nudge_targets_height(true, true)); // 6 -> 7
        assert!(
            !s.nudge_targets_height(true, true),
            "above max must not dirty"
        );
        assert_eq!(s.targets_height, 7);
    }
}
