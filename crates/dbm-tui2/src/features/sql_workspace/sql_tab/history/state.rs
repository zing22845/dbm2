//! History feature state.
//!
//! Owns the UI state for the history list: the `/` search state, the list
//! cursor/scroll offsets, and the detail sub-pane state.
//!
//! The SQL history *store* (`SqlHistoryStore`) lives on the parent
//! `SqlTabState` (per connection, shared across all tabs). It is passed as a
//! read-only parameter to view/update functions rather than stored here.

use crate::common::components::search::PaneSearch;

use super::detail::HistoryDetailState;

/// The history feature state (the history list pane + detail preview).
#[derive(Debug, Clone)]
#[derive(Default)]
pub struct HistoryState {
    /// The `/` search state for the history list.
    pub search: PaneSearch,
    /// Cursor into the (filtered) history list.
    pub cursor: usize,
    /// Vertical scroll offset of the list.
    pub v_scroll: usize,
    /// Horizontal scroll offset of the list rows.
    pub h_scroll: usize,
    /// The detail sub-pane state.
    pub detail: HistoryDetailState,
    /// The History-internal detail/list splitter child feature (B).
    pub splitter: super::splitter::state::DetailSplitterState,
}


impl HistoryState {
    /// Indices of history entries matching the current filter.
    pub fn visible_indices(
        &self,
        store: &super::store::SqlHistoryStore,
        instance: &str,
        connection: &str,
    ) -> Vec<usize> {
        super::store::history_visible_indices(store.entries(instance, connection), &self.search)
    }

    /// The currently selected history entry, if any.
    pub fn selected_entry(
        &self,
        store: &super::store::SqlHistoryStore,
        instance: &str,
        connection: &str,
    ) -> Option<String> {
        let entries = store.entries(instance, connection);
        let visible = self.visible_indices(store, instance, connection);
        let &idx = visible.get(self.cursor)?;
        entries.get(idx).cloned()
    }

    /// Pin the most recent entry and place the cursor on it, mirroring the
    /// original dbm's `enter_history_recall_from_sql`: the recall list opens on
    /// the newest statement with its detail shown.
    pub fn pin_most_recent(
        &mut self,
        store: &super::store::SqlHistoryStore,
        instance: &str,
        connection: &str,
    ) {
        let visible = self.visible_indices(store, instance, connection);
        if let Some(&first) = visible.first() {
            self.cursor = 0;
            let entries = store.entries(instance, connection);
            if let Some(sql) = entries.get(first) {
                self.detail.pinned_sql = Some(sql.clone());
            }
        }
    }
}
