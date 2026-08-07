//! Discovery results feature update.

use super::msg::ResultsMessage;
use super::state::ResultsState;
use super::intent::ResultsIntent;
use super::effect::ResultsEffect;

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
        ResultsMessage::MoveUp => state.move_up(),
        ResultsMessage::MoveDown => state.move_down(),
        ResultsMessage::ToggleSelect => {
            state.toggle_select();
            true
        }
        ResultsMessage::ToggleUnregisteredFilter => {
            state.unregistered_only = !state.unregistered_only;
            true
        }
    };
    (state, Vec::new(), Vec::new(), dirty)
}
