//! Discovery results feature update.

use super::msg::ResultsMessage;
use super::state::ResultsState;
use super::intent::ResultsIntent;
use super::effect::ResultsEffect;

pub fn update(
    _msg: ResultsMessage,
    _state: ResultsState,
) -> (ResultsState, Vec<ResultsIntent>, Vec<ResultsEffect>) {
    match _msg {}
}
