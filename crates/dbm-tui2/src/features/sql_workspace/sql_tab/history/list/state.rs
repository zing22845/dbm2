//! History list sub-feature state.
//!
//! Owns the list cursor/scroll/search — all the UI state of the SQL history
//! entry list. The actual SQL entries live in the shared `SqlHistoryStore`
//! (on the parent `SqlTabState`, per connection); they are read-only params
//! to the view/update helpers rather than stored here.

use crate::common::components::search::PaneSearch;

use super::super::store::SqlHistoryStore;

/// State for the history list sub-feature.
#[derive(Debug, Clone, Default)]
pub struct ListState {
    /// The `/` search state for the history list.
    pub search: PaneSearch,
    /// Cursor into the (filtered) history list.
    pub cursor: usize,
    /// Vertical scroll offset of the list (discover-style viewport start row).
    pub v_scroll: usize,
    /// Horizontal scroll offset of the list rows.
    pub h_scroll: usize,
}

impl ListState {
    /// Indices of history entries matching the current filter.
    pub fn visible_indices(
        &self,
        store: &SqlHistoryStore,
        instance: &str,
        connection: &str,
    ) -> Vec<usize> {
        super::super::store::history_visible_indices(store.entries(instance, connection), &self.search)
    }

    /// The currently selected history entry, if any.
    pub fn selected_entry(
        &self,
        store: &SqlHistoryStore,
        instance: &str,
        connection: &str,
    ) -> Option<String> {
        let entries = store.entries(instance, connection);
        let visible = self.visible_indices(store, instance, connection);
        let &idx = visible.get(self.cursor)?;
        entries.get(idx).cloned()
    }

    /// Generous upper bound for h_scroll of the currently selected entry.
    /// The real, viewport-aware max is computed in the renderer as
    /// `line_width - content_w`; the update layer returns `line_width` which
    /// is always ≥ the view's maximum, and the renderer clamps further.
    pub fn max_h_scroll(
        &self,
        store: &SqlHistoryStore,
        instance: &str,
        connection: &str,
    ) -> usize {
        let visible = self.visible_indices(store, instance, connection);
        let entries = store.entries(instance, connection);
        visible
            .get(self.cursor)
            .and_then(|&idx| entries.get(idx))
            .map(|sql| super::super::store::history_line_display_width(sql) as usize)
            .unwrap_or(0)
    }
}
