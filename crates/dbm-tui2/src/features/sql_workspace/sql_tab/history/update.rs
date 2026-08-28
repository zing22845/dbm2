//! History feature update (parent).
//!
//! Routes `HistoryMessage` variants to the appropriate child feature
//! update (list or detail) and handles cross-cutting concerns:
//! - `RecordSuccess` stays at parent (needs shared SqlHistoryStore write).
//! - `Apply` stays at parent (emits `Recall` intent, cursor→detail reconcile).
//! - List navigation / search / scroll → `list::update`.
//! - Detail scroll → `detail::update`.
//! - After any cursor-affecting list message, reconcile detail scroll to
//!   the newly selected entry (cross-feature coordination).

use super::detail::state::DetailState;
use super::detail::update as detail_update;
use super::effect::HistoryEffect;
use super::intent::HistoryIntent;
use super::list::state::ListState;
use super::list::update as list_update;
use super::msg::HistoryMessage;
use super::state::HistoryState;
use super::store::SqlHistoryStore;

/// Update the history feature state. Pure by-value transition.
///
/// The returned `bool` is `dirty`: whether the rendered history list or
/// detail changed. Cursor/detail navigation reports `false` at a boundary;
/// `Apply` only pushes a recall intent (the editor update handles the change).
///
/// Note: `RecordSuccess` is intercepted by the parent `SqlTabState::update`
/// because the store is shared across all tabs. Only UI-affecting messages
/// (MoveCursor, SearchKey, Apply, ScrollDetail) are processed here.
#[allow(clippy::too_many_arguments)]
pub fn update(
    msg: HistoryMessage,
    mut state: HistoryState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
    // The sql of the currently selected entry (for detail scroll reconciliation).
    selected_sql: Option<String>,
    detail_text_width_after: u16,
    detail_viewport: usize,
) -> (HistoryState, Vec<HistoryIntent>, Vec<HistoryEffect>, bool) {
    let mut intents = Vec::new();
    let effects = Vec::new();

    let dirty = match msg {
        HistoryMessage::RecordSuccess { .. } => {
            // Handled by parent: store is shared at SqlTabState level.
            true
        }
        // --- List messages: delegate to list::update, then reconcile detail ---
        HistoryMessage::MoveCursor { delta } => {
            let (new_list, list_dirty) = list_update::update(
                super::list::msg::ListMessage::MoveCursor { delta },
                state.list,
                store,
                instance,
                connection,
            );
            state.list = new_list;
            reconcile_detail_after_cursor_change(&mut state.detail, &state.list, store, instance, connection, detail_text_width_after, detail_viewport);
            list_dirty
        }
        HistoryMessage::SetCursor { index } => {
            let (new_list, list_dirty) = list_update::update(
                super::list::msg::ListMessage::SetCursor { index },
                state.list,
                store,
                instance,
                connection,
            );
            state.list = new_list;
            reconcile_detail_after_cursor_change(&mut state.detail, &state.list, store, instance, connection, detail_text_width_after, detail_viewport);
            list_dirty
        }
        HistoryMessage::BeginSearch => {
            let (new_list, list_dirty) = list_update::update(
                super::list::msg::ListMessage::BeginSearch,
                state.list,
                store,
                instance,
                connection,
            );
            state.list = new_list;
            reconcile_detail_after_cursor_change(&mut state.detail, &state.list, store, instance, connection, detail_text_width_after, detail_viewport);
            list_dirty
        }
        HistoryMessage::SearchKey(key) => {
            let (new_list, list_dirty) = list_update::update(
                super::list::msg::ListMessage::SearchKey(key),
                state.list,
                store,
                instance,
                connection,
            );
            state.list = new_list;
            reconcile_detail_after_cursor_change(&mut state.detail, &state.list, store, instance, connection, detail_text_width_after, detail_viewport);
            list_dirty
        }
        HistoryMessage::ScrollHScroll { delta } => {
            let (new_list, list_dirty) = list_update::update(
                super::list::msg::ListMessage::ScrollHScroll { delta },
                state.list,
                store,
                instance,
                connection,
            );
            state.list = new_list;
            list_dirty
        }
        HistoryMessage::SetHScroll { position } => {
            let (new_list, list_dirty) = list_update::update(
                super::list::msg::ListMessage::SetHScroll { position },
                state.list,
                store,
                instance,
                connection,
            );
            state.list = new_list;
            list_dirty
        }
        HistoryMessage::SetVScroll { position } => {
            let (new_list, list_dirty) = list_update::update(
                super::list::msg::ListMessage::SetVScroll { position },
                state.list,
                store,
                instance,
                connection,
            );
            state.list = new_list;
            list_dirty
        }
        // --- Cross-cutting: Apply needs list state + emits intent ---
        HistoryMessage::Apply => {
            if let Some(sql) = state.list.selected_entry(store, instance, connection) {
                intents.push(HistoryIntent::Recall { sql });
            }
            false
        }
        // --- Detail messages: delegate to detail::update ---
        HistoryMessage::ScrollDetail { delta } => {
            let (new_detail, detail_dirty) = detail_update::update(
                super::detail::msg::DetailMessage::Scroll { delta },
                state.detail,
                selected_sql.as_deref().unwrap_or(""),
                detail_text_width_after,
                detail_viewport,
            );
            state.detail = new_detail;
            detail_dirty
        }
        HistoryMessage::ScrollDetailPage { down } => {
            let (new_detail, detail_dirty) = detail_update::update(
                super::detail::msg::DetailMessage::ScrollPage { down },
                state.detail,
                selected_sql.as_deref().unwrap_or(""),
                detail_text_width_after,
                detail_viewport,
            );
            state.detail = new_detail;
            detail_dirty
        }
    };

    (state, intents, effects, dirty)
}

/// Cross-feature reconciliation: after a list cursor change, scroll the
/// detail preview to show the selected entry (and jump to first search
/// match when filtered).
fn reconcile_detail_after_cursor_change(
    detail: &mut DetailState,
    list: &ListState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
    text_width_after: u16,
    viewport_lines: usize,
) {
    if let Some(sql) = list.selected_entry(store, instance, connection) {
        detail_update::reconcile_on_selection_change(
            detail,
            &sql,
            &list.search,
            text_width_after,
            viewport_lines,
        );
    }
}
