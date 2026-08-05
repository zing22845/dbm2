//! Results detail sub-module state.
//!
//! The detail section previews the selected cell's value (read-only for now;
//! the inline edit flow is a later migration). It keeps only the scroll offset.

pub const DEFAULT_DETAIL_PANE_WIDTH: u16 = 40;
pub const MIN_DETAIL_PANE_WIDTH: u16 = 24;
pub const MAX_DETAIL_PANE_WIDTH: u16 = 72;

#[derive(Debug, Default, Clone)]
pub struct DetailState {
    /// Vertical scroll offset of the detail body.
    pub scroll: usize,
}

impl DetailState {
    /// Clamp the scroll to the number of wrapped display rows.
    pub fn clamp_scroll(&mut self, row_count: usize, viewport: usize) {
        let max = row_count.saturating_sub(viewport.max(1));
        self.scroll = self.scroll.min(max);
    }
}

pub fn clamp_detail_pane_width(width: u16) -> u16 {
    width.clamp(MIN_DETAIL_PANE_WIDTH, MAX_DETAIL_PANE_WIDTH)
}
