//! State of the explorer instances/objects horizontal splitter.

/// Default instances editor height in rows (the top pane). The layout clamps it
/// to `[20%, 80%]` of the current track anyway.
pub const DEFAULT_INSTANCES_HEIGHT: u16 = 10;

/// State of the explorer instances/objects splitter. The instances tree height
/// is stored in **absolute rows** so dragging records the exact row under the
/// pointer (no percent round-trip loss); it is clamped to `[20%, 80%]` of the
/// current track at layout time. The percentage is only materialized for
/// session persistence / terminal resizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplorerSplitterState {
    /// Instances tree (top pane) height in rows.
    pub instances_height: u16,
    /// The actual instances-height bounds (rows) the last layout clamped to,
    /// refreshed by the run loop from the layout; keyboard nudges clamp to these
    /// so the stored value and the rendered split never disagree.
    pub instances_min: u16,
    pub instances_max: u16,
}

impl Default for ExplorerSplitterState {
    fn default() -> Self {
        Self {
            instances_height: DEFAULT_INSTANCES_HEIGHT,
            instances_min: 1,
            instances_max: 200,
        }
    }
}

impl ExplorerSplitterState {
    /// Set and clamp the instances height in rows (drag the splitter). Returns
    /// `true` when the stored height actually changed, so callers can skip a
    /// redundant repaint.
    pub fn set_instances_height(&mut self, height: u16) -> bool {
        let new = height.clamp(1, 1000);
        if new == self.instances_height {
            return false;
        }
        self.instances_height = new;
        true
    }

    /// The instances height as a percentage of the given track height (rows),
    /// used only for session persistence / terminal resizes.
    pub fn instances_height_pct(&self, track_h: u16) -> u8 {
        if track_h == 0 {
            return 20;
        }
        let pct = (u32::from(self.instances_height) * 100) / u32::from(track_h);
        pct.clamp(20, 80) as u8
    }

    /// Set the instances height from a percentage of the given track height
    /// (rows), used when restoring a persisted session whose stored split is a
    /// percentage.
    pub fn set_instances_height_pct(&mut self, pct: u8, track_h: u16) {
        let rows = (u32::from(track_h) * u32::from(pct.clamp(20, 80))) / 100;
        self.instances_height = rows.clamp(1, 1000) as u16;
    }

    /// Nudge the instances height by one keyboard step. `plus` is the `+` / `-`
    /// key (true = `+`); `top_focused` is whether the instances (top) pane is
    /// focused rather than the objects (bottom). `+` always grows the focused
    /// pane and `-` shrinks it. Returns `true` when the split actually moved.
    pub fn nudge_instances_height(&mut self, plus: bool, top_focused: bool) -> bool {
        use crate::common::view::splitter::WIDTH_NUDGE_STEP;
        let grow_top = if plus { top_focused } else { !top_focused };
        let delta = if grow_top { WIDTH_NUDGE_STEP } else { -WIDTH_NUDGE_STEP };
        // Clamp to the layout's actual bounds so nudging past the boundary
        // leaves the stored height unchanged (no redundant repaint).
        let next = (self.instances_height as i16 + delta)
            .clamp(self.instances_min as i16, self.instances_max as i16)
            as u16;
        self.set_instances_height(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_height_only_marks_dirty_on_change() {
        let mut s = ExplorerSplitterState::default();
        assert!(s.set_instances_height(15));
        assert_eq!(s.instances_height, 15);
        assert!(!s.set_instances_height(15), "unchanged height must not dirty");
        assert!(s.set_instances_height(16));
    }

    #[test]
    fn pct_round_trip() {
        let mut s = ExplorerSplitterState::default();
        s.set_instances_height_pct(50, 20);
        assert_eq!(s.instances_height, 10);
        assert_eq!(s.instances_height_pct(20), 50);
    }

    #[test]
    fn nudge_grows_focused_pane() {
        let mut s = ExplorerSplitterState::default();
        s.instances_height = 20;
        // + with instances focused grows the top.
        assert!(s.nudge_instances_height(true, true));
        assert_eq!(s.instances_height, 22);
        // + with objects focused shrinks the top (grows the bottom).
        let mut s2 = ExplorerSplitterState::default();
        s2.instances_height = 20;
        assert!(s2.nudge_instances_height(true, false));
        assert_eq!(s2.instances_height, 18);
        // - with instances focused shrinks the top.
        let mut s3 = ExplorerSplitterState::default();
        s3.instances_height = 20;
        assert!(s3.nudge_instances_height(false, true));
        assert_eq!(s3.instances_height, 18);
        // - with objects focused grows the top.
        let mut s4 = ExplorerSplitterState::default();
        s4.instances_height = 20;
        assert!(s4.nudge_instances_height(false, false));
        assert_eq!(s4.instances_height, 22);
    }

    #[test]
    fn nudge_stops_dirtying_at_the_boundary() {
        let mut s = ExplorerSplitterState::default();
        s.instances_min = 2;
        s.instances_max = 7;
        s.instances_height = 3;
        assert!(s.nudge_instances_height(false, true)); // 3 -> 2
        assert_eq!(s.instances_height, 2);
        assert!(!s.nudge_instances_height(false, true), "below min must not dirty");
        assert_eq!(s.instances_height, 2);
        s.instances_height = 6;
        assert!(s.nudge_instances_height(true, true)); // 6 -> 7
        assert!(!s.nudge_instances_height(true, true), "above max must not dirty");
        assert_eq!(s.instances_height, 7);
    }
}
