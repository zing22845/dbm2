//! Results detail sub-module state.
//!
//! The detail section previews the selected cell's value. It owns the scroll
//! offset, pane width, and the inline-edit draft state.

pub const DEFAULT_DETAIL_PANE_WIDTH: u16 = 40;
pub const MIN_DETAIL_PANE_WIDTH: u16 = 24;
pub const MAX_DETAIL_PANE_WIDTH: u16 = 72;

#[derive(Debug, Clone)]
pub struct DetailState {
    /// Vertical scroll offset of the detail body.
    pub scroll: usize,
    /// Width of the detail pane in columns (used when detail is open).
    pub pane_width: u16,
    /// The detail draft's baseline (cell value at load) for dirty detection.
    pub baseline: String,
    /// The detail draft's current text (edited value).
    pub draft: String,
    /// Whether the detail draft is dirty (differs from baseline).
    pub dirty: bool,
    /// Whether an unsaved detail draft blocks leaving Detail.
    pub leave_warning: bool,
}

impl Default for DetailState {
    fn default() -> Self {
        Self {
            scroll: 0,
            pane_width: DEFAULT_DETAIL_PANE_WIDTH,
            baseline: String::new(),
            draft: String::new(),
            dirty: false,
            leave_warning: false,
        }
    }
}

impl DetailState {
    /// Clamp the scroll to the number of wrapped display rows.
    pub fn clamp_scroll(&mut self, row_count: usize, viewport: usize) {
        let max = row_count.saturating_sub(viewport.max(1));
        self.scroll = self.scroll.min(max);
    }

    /// Set the detail pane width, clamped to valid bounds.
    pub fn set_pane_width(&mut self, width: u16) {
        self.pane_width = clamp_detail_pane_width(width);
    }

    /// Load a cell value as the draft baseline (used when entering edit).
    pub fn load_cell(&mut self, value: &str) {
        self.baseline = value.to_string();
        self.draft = value.to_string();
        self.dirty = false;
    }

    /// Clear the draft state (used when exiting edit or rolling back).
    pub fn clear_draft(&mut self) {
        self.baseline.clear();
        self.draft.clear();
        self.dirty = false;
        self.leave_warning = false;
    }

    /// Reset scroll and leave warning (used when closing detail).
    pub fn reset_for_close(&mut self) {
        self.scroll = 0;
        self.leave_warning = false;
    }
}

pub fn clamp_detail_pane_width(width: u16) -> u16 {
    width.clamp(MIN_DETAIL_PANE_WIDTH, MAX_DETAIL_PANE_WIDTH)
}