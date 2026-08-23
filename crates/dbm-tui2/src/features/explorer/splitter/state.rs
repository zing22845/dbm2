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
}

impl Default for ExplorerSplitterState {
    fn default() -> Self {
        Self {
            instances_height: DEFAULT_INSTANCES_HEIGHT,
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
}
