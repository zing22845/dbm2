//! History detail sub-feature state.
//!
//! Owns the vertical scroll offset of the detail preview pane and the
//! optional pinned SQL entry (opened when entering recall mode before any
//! selection). The detail pane width lives in the `splitter` sub-feature.

#[derive(Debug, Default, Clone)]
pub struct DetailState {
    /// Vertical scroll offset of the detail body.
    pub scroll: usize,
    /// The entry whose detail is pinned open (set when entering recall or
    /// selecting an entry). Mirrors the original dbm's `detail_pinned`.
    pub pinned_sql: Option<String>,
}

impl DetailState {
    /// Clamp the scroll to the total wrapped display rows minus viewport.
    pub fn clamp_scroll(&mut self, row_count: usize, viewport: usize) {
        let max = row_count.saturating_sub(viewport.max(1));
        self.scroll = self.scroll.min(max);
    }

    /// Reset scroll and pinned SQL (used when closing detail or clearing store).
    pub fn reset(&mut self) {
        self.scroll = 0;
        self.pinned_sql = None;
    }

    /// Set a pinned SQL entry (used when entering history recall from the editor).
    pub fn pin(&mut self, sql: String) {
        self.pinned_sql = Some(sql);
    }
}
