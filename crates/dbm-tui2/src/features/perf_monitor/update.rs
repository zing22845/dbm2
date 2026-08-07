//! Performance monitor feature update.

use super::msg::PerfMessage;
use super::state::PerfState;
use super::intent::PerfIntent;
use super::effect::PerfEffect;

pub fn update(
    _msg: PerfMessage,
    _state: &mut PerfState,
) -> (PerfState, Vec<PerfIntent>, Vec<PerfEffect>, bool) {
    // `PerfMessage` is an empty enum: the match arm is diverging (never), so
    // this update is unreachable. It always reports `false` for dirty — the
    // perf monitor is a passive measurement driven by the run loop, not by
    // messages, and never changes rendered state via this path.
    match _msg {}
}
