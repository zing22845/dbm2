//! History feature update.

use super::msg::HistoryMessage;
use super::state::HistoryState;
use super::intent::HistoryIntent;
use super::effect::HistoryEffect;

pub fn update(
    _msg: HistoryMessage,
    _state: HistoryState,
) -> (HistoryState, Vec<HistoryIntent>, Vec<HistoryEffect>) {
    match _msg {}
}
