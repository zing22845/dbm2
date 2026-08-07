//! Engine selector feature update.

use super::msg::EngineMessage;
use super::state::EngineState;
use super::intent::EngineIntent;
use super::effect::EngineEffect;

/// Update the engine selector state. Pure by-value transition.
///
/// The returned `bool` is `dirty`: selecting an engine always changes the
/// rendered engine selector.
pub fn update(
    msg: EngineMessage,
    mut state: EngineState,
) -> (EngineState, Vec<EngineIntent>, Vec<EngineEffect>, bool) {
    match msg {
        EngineMessage::Select(engine) => state.engine = engine,
    }
    (state, Vec::new(), Vec::new(), true)
}
