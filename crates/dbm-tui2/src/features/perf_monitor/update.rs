//! Performance monitor feature update.

use super::msg::PerfMessage;
use super::state::PerfState;
use super::intent::PerfIntent;
use super::effect::PerfEffect;

pub fn update(
    _msg: PerfMessage,
    _state: &mut PerfState,
) -> (PerfState, Vec<PerfIntent>, Vec<PerfEffect>) {
    match _msg {}
}
