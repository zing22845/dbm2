//! Engine selector feature update.

use super::msg::EngineMessage;
use super::state::EngineState;
use super::intent::EngineIntent;
use super::effect::EngineEffect;

pub fn update(
    _msg: EngineMessage,
    _state: EngineState,
) -> (EngineState, Vec<EngineIntent>, Vec<EngineEffect>) {
    match _msg {}
}
