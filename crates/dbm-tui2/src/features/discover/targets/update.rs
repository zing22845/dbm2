//! Discovery targets editor feature update.

use super::msg::TargetsMessage;
use super::state::TargetsState;
use super::intent::TargetsIntent;
use super::effect::TargetsEffect;

pub fn update(
    _msg: TargetsMessage,
    _state: TargetsState,
) -> (TargetsState, Vec<TargetsIntent>, Vec<TargetsEffect>) {
    match _msg {}
}
