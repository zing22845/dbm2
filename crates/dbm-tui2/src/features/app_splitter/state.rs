//! State owned by the app-level splitter feature: the width of the Explorer
//! pane (left) vs the workspace region (right). The splitter separates two
//! peer top-level features (explorer and the workspace), so — mirroring the
//! `sql_tab` splitter which lives at the common parent of editor/history — this
//! splitter is its own app-level feature.

/// Width range of the Explorer pane (the splitter clamps to this).
pub const MIN_EXPLORER_WIDTH: u16 = 16;
pub const MAX_EXPLORER_WIDTH: u16 = 80;
/// Default Explorer pane width (columns).
pub const DEFAULT_EXPLORER_WIDTH: u16 = 24;

/// State of the app's single resizable vertical splitter: explorer vs the
/// workspace region. The width is stored as absolute columns, matching the
/// editor/history and detail/list splitters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppSplitterState {
    /// Width of the Explorer pane (the left side of the splitter).
    pub explorer_pane_width: u16,
}

impl Default for AppSplitterState {
    fn default() -> Self {
        Self {
            explorer_pane_width: DEFAULT_EXPLORER_WIDTH,
        }
    }
}

impl AppSplitterState {
    /// Set and clamp the Explorer pane width (drag the splitter).
    /// Set and clamp the Explorer pane width (drag the splitter). Returns
    /// `true` when the width actually changed, so callers can skip a redundant
    /// repaint when the split is already at a boundary.
    pub fn set_explorer_pane_width(&mut self, width: u16) -> bool {
        let new = width.clamp(MIN_EXPLORER_WIDTH, MAX_EXPLORER_WIDTH);
        if new == self.explorer_pane_width {
            return false;
        }
        self.explorer_pane_width = new;
        true
    }

    /// Nudge the Explorer pane width by `delta` columns (the `[` / `]` keys),
    /// clamped to the allowed range. The Explorer owns the left side, so `]`
    /// grows it and `[` shrinks it.
    pub fn nudge_explorer_width(&mut self, delta: i16) {
        let next = self.explorer_pane_width as i16 + delta;
        self.explorer_pane_width = next.clamp(MIN_EXPLORER_WIDTH as i16, MAX_EXPLORER_WIDTH as i16) as u16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_width_is_24() {
        assert_eq!(AppSplitterState::default().explorer_pane_width, 24);
    }

    #[test]
    fn set_width_clamps_to_range() {
        let mut s = AppSplitterState::default();
        s.set_explorer_pane_width(10);
        assert_eq!(s.explorer_pane_width, MIN_EXPLORER_WIDTH);
        s.set_explorer_pane_width(500);
        assert_eq!(s.explorer_pane_width, MAX_EXPLORER_WIDTH);
        s.set_explorer_pane_width(40);
        assert_eq!(s.explorer_pane_width, 40);
    }

    #[test]
    fn nudge_grows_and_shrinks() {
        let mut s = AppSplitterState::default();
        s.nudge_explorer_width(2);
        assert_eq!(s.explorer_pane_width, 26);
        s.nudge_explorer_width(-4);
        assert_eq!(s.explorer_pane_width, 22);
        s.nudge_explorer_width(-100);
        assert_eq!(s.explorer_pane_width, MIN_EXPLORER_WIDTH);
        s.nudge_explorer_width(1000);
        assert_eq!(s.explorer_pane_width, MAX_EXPLORER_WIDTH);
    }
}
