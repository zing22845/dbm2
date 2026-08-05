//! Discovery results feature update.

use super::msg::ResultsMessage;
use super::state::ResultsState;
use super::intent::ResultsIntent;
use super::effect::ResultsEffect;

/// Update the results list state. Pure by-value transition.
pub fn update(
    msg: ResultsMessage,
    mut state: ResultsState,
) -> (ResultsState, Vec<ResultsIntent>, Vec<ResultsEffect>) {
    match msg {
        ResultsMessage::MoveUp => {
            state.move_up();
        }
        ResultsMessage::MoveDown => {
            state.move_down();
        }
        ResultsMessage::ToggleSelect => {
            state.toggle_select();
        }
        ResultsMessage::ToggleUnregisteredFilter => {
            state.unregistered_only = !state.unregistered_only;
        }
    }
    (state, Vec::new(), Vec::new())
}
