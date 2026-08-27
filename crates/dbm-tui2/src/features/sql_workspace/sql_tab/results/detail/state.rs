//! Results detail sub-module state.
//!
//! The detail section previews the selected cell's value. It owns the scroll
//! offset and the inline-edit draft state. The detail pane width lives in the
//! `splitter` sub-feature (`super::splitter::state`).

#[derive(Debug, Clone, Default)]
pub struct DetailState {
    /// Vertical scroll offset of the detail body.
    pub scroll: usize,
    /// The detail draft's baseline (cell value at load) for dirty detection.
    pub baseline: String,
    /// The detail draft's current text (edited value).
    pub draft: String,
    /// Whether the detail draft is dirty (differs from baseline).
    pub dirty: bool,
    /// Whether an unsaved detail draft blocks leaving Detail.
    pub leave_warning: bool,
}

impl DetailState {
    /// Clamp the scroll to the number of wrapped display rows.
    pub fn clamp_scroll(&mut self, row_count: usize, viewport: usize) {
        let max = row_count.saturating_sub(viewport.max(1));
        self.scroll = self.scroll.min(max);
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
