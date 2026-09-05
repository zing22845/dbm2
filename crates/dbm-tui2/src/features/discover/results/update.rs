//! Discovery results feature update.

use super::effect::ResultsEffect;
use super::intent::ResultsIntent;
use super::msg::ResultsMessage;
use super::state::ResultsState;

/// Update the results list state. Pure by-value transition.
///
/// The returned `bool` is `dirty`: whether the rendered results list changed.
/// `MoveUp`/`MoveDown` report `false` when clamped at a boundary; the other
/// messages always change state.
pub fn update(
    msg: ResultsMessage,
    mut state: ResultsState,
) -> (ResultsState, Vec<ResultsIntent>, Vec<ResultsEffect>, bool) {
    let dirty = match msg {
        ResultsMessage::MoveUp => {
            // Cursor move clears scroll_locked — user is navigating again.
            state.scroll_locked = false;
            state.move_up()
        }
        ResultsMessage::MoveDown => {
            state.scroll_locked = false;
            state.move_down()
        }
        ResultsMessage::ToggleSelect => state.toggle_select(),
        ResultsMessage::ToggleUnregisteredFilter => state.toggle_unregistered_filter(),
        ResultsMessage::SetVScroll { position } => {
            let total = state.row_count();
            // The actual clamp to max_scroll happens at render time via the
            // viewport computation; here we only clamp to total to avoid panic
            // on an empty list or tiny row_count.
            let clamped = position.min(total.saturating_sub(1));
            let changed = state.scroll != clamped;
            state.scroll = clamped;
            state.scroll_locked = true;
            changed
        }
        ResultsMessage::SetCursor { row } => {
            state.scroll_locked = false;
            let total = state.row_count();
            let clamped = row.min(total.saturating_sub(1));
            let changed = state.cursor != clamped;
            state.cursor = clamped;
            changed
        }
    };
    (state, Vec::new(), Vec::new(), dirty)
}
