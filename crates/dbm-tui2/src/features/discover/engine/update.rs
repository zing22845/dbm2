//! Engine selector feature update.

use super::msg::EngineMessage;
use super::state::EngineState;
use super::intent::EngineIntent;
use super::effect::EngineEffect;

/// Update the engine selector state. Pure by-value transition.
pub fn update(
    msg: EngineMessage,
    mut state: EngineState,
) -> (EngineState, Vec<EngineIntent>, Vec<EngineEffect>) {
    match msg {
        EngineMessage::Select(engine) => state.engine = engine,
    }
    (state, Vec::new(), Vec::new())
}
