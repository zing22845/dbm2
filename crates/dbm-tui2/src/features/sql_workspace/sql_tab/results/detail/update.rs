//! Results detail sub-module update.

use super::msg::DetailMessage;
use super::state::DetailState;
use super::intent::DetailIntent;
use super::effect::DetailEffect;

pub fn update(
    _msg: DetailMessage,
    _state: DetailState,
) -> (DetailState, Vec<DetailIntent>, Vec<DetailEffect>) {
    match _msg {}
}
