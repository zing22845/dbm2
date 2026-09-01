pub mod detail;
pub mod effect;
pub mod intent;
pub mod list;
pub mod msg;
pub mod splitter;
pub mod state;
pub mod store;
pub mod update;
pub mod view;

use list::state::ListState;
use store::SqlHistoryStore;

/// Whether the History detail preview should be shown.
///
/// The History pane must be focused, there must be stored entries, AND at
/// least one entry must be visible under the current search filter. The final
/// condition collapses the detail (like the original dbm) when a search
/// matches nothing.
pub fn detail_visible(
    history_focused: bool,
    list: &ListState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
) -> bool {
    history_focused
        && !store.entries(instance, connection).is_empty()
        && !list.visible_indices(store, instance, connection).is_empty()
}
