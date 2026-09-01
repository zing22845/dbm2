//! History list sub-feature update.
//!
//! Pure by-value transition over `ListState`. Cross-feature effects (such
//! as reconciling the detail scroll after a cursor move, or emitting the
//! `Apply` recall intent) are handled by the parent `history::update` —
//! this module never touches detail state.

use crate::common::components::search::PaneSearchInput;

use super::msg::ListMessage;
use super::state::ListState;
use super::super::store::SqlHistoryStore;

/// Update the list state. Pure by-value transition.
///
/// Returns `(new_state, dirty)` — `dirty` indicates whether the rendered
/// list changed so the parent can decide whether to redraw.
#[allow(clippy::too_many_arguments)]
pub fn update(
    msg: ListMessage,
    mut state: ListState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
) -> (ListState, bool) {
    let dirty = match msg {
        ListMessage::MoveCursor { delta } => {
            state.scroll_locked = false;
            move_cursor(&mut state, store, instance, connection, delta)
        }
        ListMessage::SetCursor { index } => {
            state.scroll_locked = false;
            set_cursor(&mut state, store, instance, connection, index)
        }
        ListMessage::BeginSearch => {
            // Re-enter search editing on the existing filter, preserving the
            // previous keyword like the original dbm (only `start`, no clear).
            state.search.start();
            state.scroll_locked = false;
            true
        }
        ListMessage::SearchKey(key) => {
            handle_search_key(&mut state, store, instance, connection, key);
            true
        }
        ListMessage::ScrollHScroll { delta } => scroll_hscroll(&mut state, store, instance, connection, delta),
        ListMessage::SetHScroll { position } => {
            state.scroll_locked = true;
            set_hscroll(&mut state, store, instance, connection, position)
        }
        ListMessage::SetVScroll { position } => {
            state.scroll_locked = true;
            set_vscroll(&mut state, store, instance, connection, position)
        }
    };
    (state, dirty)
}

fn move_cursor(
    state: &mut ListState,
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
    moved
}

fn set_cursor(
    state: &mut ListState,
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
    moved
}

fn handle_search_key(
    state: &mut ListState,
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
}

fn scroll_hscroll(
    state: &mut ListState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
    delta: i32,
) -> bool {
    let max = state.max_h_scroll(store, instance, connection);
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

fn set_hscroll(
    state: &mut ListState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
    position: usize,
) -> bool {
    let max = state.max_h_scroll(store, instance, connection);
    let before = state.h_scroll;
    state.h_scroll = position.min(max);
    state.h_scroll != before
}

fn set_vscroll(
    state: &mut ListState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
    position: usize,
) -> bool {
    let visible_len = state.visible_indices(store, instance, connection).len();
    // Generous upper bound — viewport size unknown at update time.
    let max = visible_len.saturating_sub(1);
    let before = state.v_scroll;
    state.v_scroll = position.min(max);
    state.v_scroll != before
}
