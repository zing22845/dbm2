//! History feature state.
//!
//! Composes the `list` and `detail` TEA child states plus the splitter and
//! a few convenience passthrough methods. The actual SQL history *store*
//! (`SqlHistoryStore`) lives on the parent `SqlTabState` (per connection,
//! shared across all tabs) — passed as read-only to view/update helpers.

use super::detail::state::DetailState;
use super::list::state::ListState;

/// The history feature state (the history list pane + detail preview + splitter).
#[derive(Debug, Clone, Default)]
pub struct HistoryState {
    /// The list sub-feature state (cursor, scroll, search).
    pub list: ListState,
    /// The detail sub-feature state (scroll, pinned SQL).
    pub detail: DetailState,
    /// The History-internal detail/list splitter child feature.
    pub splitter: super::splitter::state::DetailSplitterState,
}

impl HistoryState {
    /// Build a fresh state.
    pub fn new() -> Self {
        HistoryState::default()
    }

    /// Pin the most recent entry and place the cursor on it, mirroring the
    /// original dbm's `enter_history_recall_from_sql`: the recall list opens
    /// on the newest statement with its detail shown.
    pub fn pin_most_recent(
        &mut self,
        store: &super::store::SqlHistoryStore,
        instance: &str,
        connection: &str,
    ) {
        let visible = self.list.visible_indices(store, instance, connection);
        if let Some(&first) = visible.first() {
            self.list.cursor = 0;
            let entries = store.entries(instance, connection);
            if let Some(sql) = entries.get(first) {
                self.detail.pin(sql.clone());
            }
        }
    }
}
