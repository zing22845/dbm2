//! History feature update.
//!
//! Pure by-value transition over the history list + detail state. Recording,
//! cursor movement, `/` search, apply (recall intent) and detail scrolling are
//! all pure; the list selection and detail scroll are reconciled against the
//! available history for the tab's connection.

use crate::common::components::search::PaneSearchInput;

use super::msg::HistoryMessage;
use super::state::HistoryState;
use super::intent::HistoryIntent;
use super::effect::HistoryEffect;
use super::detail::{clamp_detail_scroll, scroll_on_selection_change};

/// Update the history feature state. Pure by-value transition.
pub fn update(
    msg: HistoryMessage,
    mut state: HistoryState,
    instance: &str,
    connection: &str,
    // The sql of the currently selected entry (for detail scroll reconciliation).
    selected_sql: Option<String>,
    detail_text_width: u16,
    detail_viewport: usize,
) -> (HistoryState, Vec<HistoryIntent>, Vec<HistoryEffect>) {
    let mut intents = Vec::new();

    match msg {
        HistoryMessage::RecordSuccess { instance, connection, sql } => {
            state.store.record_success(&instance, &connection, &sql);
        }
        HistoryMessage::MoveCursor { delta } => {
            move_cursor(&mut state, instance, connection, delta);
        }
        HistoryMessage::BeginSearch => {
            state.search.reset();
            state.search.start();
        }
        HistoryMessage::SearchKey(key) => {
            handle_search_key(&mut state, instance, connection, key);
        }
        HistoryMessage::Apply => {
            if let Some(sql) = state.selected_entry(instance, connection) {
                intents.push(HistoryIntent::Recall { sql });
            }
        }
        HistoryMessage::ScrollDetail { delta } => {
            scroll_detail(&mut state, instance, connection, selected_sql.as_deref(), detail_text_width, detail_viewport, delta);
        }
        HistoryMessage::ScrollDetailPage { down } => {
            let sql = selected_sql.as_deref();
            if let Some(sql) = sql {
                super::detail::scroll_half_page(&mut state.detail, sql, detail_text_width, detail_viewport, down);
            }
        }
    }

    (state, intents, Vec::new())
}

/// Move the list cursor, clamping to the filtered entries, and reconcile the
/// detail scroll (jump to first match when filtered).
fn move_cursor(
    state: &mut HistoryState,
    instance: &str,
    connection: &str,
    delta: i32,
) {
    let visible_len = state.visible_indices(instance, connection).len();
    if visible_len == 0 {
        return;
    }
    let next = if delta > 0 {
        (state.cursor + 1).min(visible_len - 1)
    } else {
        state.cursor.saturating_sub(1)
    };
    if next != state.cursor {
        state.cursor = next;
        state.h_scroll = 0;
    }
    // Reconcile detail scroll to the newly selected entry.
    if let Some(sql) = state.selected_entry(instance, connection) {
        scroll_on_selection_change(&mut state.detail, &sql, &state.search, 40, 8);
    }
}

/// Handle a search-input key (query changes, navigation, etc.).
fn handle_search_key(
    state: &mut HistoryState,
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
        move_cursor(state, instance, connection, if forward { 1 } else { -1 });
        return;
    }

    if matches!(action, PaneSearchInput::QueryChanged | PaneSearchInput::OptionsChanged) {
        state.cursor = 0;
        state.v_scroll = 0;
    }

    if let Some(sql) = state.selected_entry(instance, connection) {
        scroll_on_selection_change(&mut state.detail, &sql, &state.search, 40, 8);
    }
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
) {
    let Some(sql) = sql else {
        return;
    };
    if delta > 0 {
        state.detail.scroll = state.detail.scroll.saturating_add(delta as usize);
    } else {
        state.detail.scroll = state.detail.scroll.saturating_sub(delta.unsigned_abs() as usize);
    }
    clamp_detail_scroll(&mut state.detail, sql, detail_text_width, detail_viewport);
}
