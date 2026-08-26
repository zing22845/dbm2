//! History feature update.
//!
//! Pure by-value transition over the history list + detail state. Recording,
//! cursor movement, `/` search, apply (recall intent) and detail scrolling are
//! all pure; the list selection and detail scroll are reconciled against the
//! available history for the tab's connection.
//!
//! The `SqlHistoryStore` is shared at the `SqlTabState` level (per connection),
//! so it is passed as a read-only parameter rather than stored in `HistoryState`.
//! `RecordSuccess` is intercepted by the parent and applied directly to the
//! shared store before routing other messages here.

use crate::common::components::search::PaneSearchInput;

use super::msg::HistoryMessage;
use super::state::HistoryState;
use super::store::SqlHistoryStore;
use super::intent::HistoryIntent;
use super::effect::HistoryEffect;
use super::detail::{clamp_detail_scroll, scroll_on_selection_change};

/// Update the history feature state. Pure by-value transition.
///
/// The returned `bool` is `dirty`: whether the rendered history list or detail
/// changed. Cursor/detail navigation reports `false` at a boundary; `Apply`
/// only pushes a recall intent (the editor update handles the change).
///
/// Note: `RecordSuccess` is handled by the parent `SqlTabState::update`
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
    detail_text_width: u16,
    detail_viewport: usize,
) -> (HistoryState, Vec<HistoryIntent>, Vec<HistoryEffect>, bool) {
    let mut intents = Vec::new();
    let effects = Vec::new();

    let dirty = match msg {
        HistoryMessage::RecordSuccess { .. } => {
            // Handled by parent: store is shared at SqlTabState level.
            true
        }
        HistoryMessage::MoveCursor { delta } => {
            move_cursor(&mut state, store, instance, connection, delta)
        }
        HistoryMessage::SetCursor { index } => {
            set_cursor(&mut state, store, instance, connection, index)
        }
        HistoryMessage::BeginSearch => {
            state.search.reset();
            state.search.start();
            true
        }
        HistoryMessage::SearchKey(key) => {
            handle_search_key(&mut state, store, instance, connection, key);
            true
        }
        HistoryMessage::Apply => {
            if let Some(sql) = state.selected_entry(store, instance, connection) {
                intents.push(HistoryIntent::Recall { sql });
            }
            false
        }
        HistoryMessage::ScrollDetail { delta } => {
            scroll_detail(&mut state, instance, connection, selected_sql.as_deref(), detail_text_width, detail_viewport, delta)
        }
        HistoryMessage::ScrollDetailPage { down } => {
            let sql = selected_sql.as_deref();
            if let Some(sql) = sql {
                let before = state.detail.scroll;
                super::detail::scroll_half_page(&mut state.detail, sql, detail_text_width, detail_viewport, down);
                state.detail.scroll != before
            } else {
                false
            }
        }
        HistoryMessage::ScrollHScroll { delta } => {
            scroll_hscroll(&mut state, store, instance, connection, delta)
        }
        HistoryMessage::SetHScroll { position } => {
            set_hscroll(&mut state, store, instance, connection, position)
        }
    };

    (state, intents, effects, dirty)
}

/// Move the list cursor, clamping to the filtered entries, and reconcile the
/// detail scroll (jump to first match when filtered).
fn move_cursor(
    state: &mut HistoryState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
    delta: i32,
) -> bool {
    let visible_len = state.visible_indices(store, instance, connection).len();
    if visible_len == 0 {
        return false;
    }
    let next = if delta > 0 {
        (state.cursor + 1).min(visible_len - 1)
    } else {
        state.cursor.saturating_sub(1)
    };
    let moved = next != state.cursor;
    if moved {
        state.cursor = next;
        state.h_scroll = 0;
    }
    // Reconcile detail scroll to the newly selected entry.
    if let Some(sql) = state.selected_entry(store, instance, connection) {
        scroll_on_selection_change(&mut state.detail, &sql, &state.search, 40, 8);
    }
    moved
}

/// Set the list cursor to an absolute visible row index (from a mouse click).
fn set_cursor(
    state: &mut HistoryState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
    index: usize,
) -> bool {
    let visible_len = state.visible_indices(store, instance, connection).len();
    if visible_len == 0 {
        return false;
    }
    let next = index.min(visible_len.saturating_sub(1));
    let moved = next != state.cursor;
    if moved {
        state.cursor = next;
        state.h_scroll = 0;
    }
    if let Some(sql) = state.selected_entry(store, instance, connection) {
        scroll_on_selection_change(&mut state.detail, &sql, &state.search, 40, 8);
    }
    moved
}

/// Handle a search-input key (query changes, navigation, etc.).
fn handle_search_key(
    state: &mut HistoryState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
    key: crossterm::event::KeyEvent,
) {
    let caps_lock = false;
    let action = match key.code {
        crossterm::event::KeyCode::Esc => {
            state.search.reset();
            PaneSearchInput::Cancelled
        }
        crossterm::event::KeyCode::Enter => {
            state.search.end();
            PaneSearchInput::Applied
        }
        _ => state.search.handle_key(&key, caps_lock),
    };

    if let PaneSearchInput::Navigate { forward } = action {
        move_cursor(state, store, instance, connection, if forward { 1 } else { -1 });
        return;
    }

    if matches!(action, PaneSearchInput::QueryChanged | PaneSearchInput::OptionsChanged) {
        state.cursor = 0;
        state.v_scroll = 0;
    }

    if let Some(sql) = state.selected_entry(store, instance, connection) {
        scroll_on_selection_change(&mut state.detail, &sql, &state.search, 40, 8);
    }
}

/// Horizontal scroll of the list rows by `delta` cells, clamped to the
/// maximum scroll width of the currently visible rows.
fn scroll_hscroll(
    state: &mut HistoryState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
    delta: i32,
) -> bool {
    let max = max_h_scroll_for_selection(state, store, instance, connection);
    let before = state.h_scroll;
    if delta > 0 {
        state.h_scroll = (state.h_scroll as u32)
            .saturating_add(delta as u32)
            .min(max as u32) as usize;
    } else {
        state.h_scroll = state.h_scroll.saturating_sub(delta.unsigned_abs() as usize);
    }
    state.h_scroll != before
}

/// Set the list horizontal scroll to an absolute position (from scrollbar drag).
fn set_hscroll(
    state: &mut HistoryState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
    position: usize,
) -> bool {
    let max = max_h_scroll_for_selection(state, store, instance, connection);
    let before = state.h_scroll;
    state.h_scroll = position.min(max);
    state.h_scroll != before
}

/// Compute the maximum horizontal scroll for the currently selected history
/// entry, based on its display width minus the visible viewport width.
fn max_h_scroll_for_selection(
    state: &HistoryState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
) -> usize {
    let visible = state.visible_indices(store, instance, connection);
    let max_idx = visible.iter().copied().max().unwrap_or(0);
    let entries = store.entries(instance, connection);
    let max_width = entries
        .get(max_idx)
        .map(|sql| super::store::history_line_display_width(sql) as usize)
        .unwrap_or(0);
    let min_viewport = 10usize;
    max_width.saturating_sub(min_viewport)
}

/// Scroll the detail preview by `delta` lines (clamped to content).
fn scroll_detail(
    state: &mut HistoryState,
    _instance: &str,
    _connection: &str,
    sql: Option<&str>,
    detail_text_width: u16,
    detail_viewport: usize,
    delta: i32,
) -> bool {
    let Some(sql) = sql else {
        return false;
    };
    let before = state.detail.scroll;
    if delta > 0 {
        state.detail.scroll = state.detail.scroll.saturating_add(delta as usize);
    } else {
        state.detail.scroll = state.detail.scroll.saturating_sub(delta.unsigned_abs() as usize);
    }
    clamp_detail_scroll(&mut state.detail, sql, detail_text_width, detail_viewport);
    state.detail.scroll != before
}
