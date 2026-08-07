//! Engine selector feature update.

use super::msg::EngineMessage;
use super::state::EngineState;
use super::intent::EngineIntent;
use super::effect::EngineEffect;

/// Update the engine selector state. Pure by-value transition.
///
/// The returned `bool` is `dirty`: whether the selected engine actually
/// changed. Re-selecting the current engine reports `false`.
pub fn update(
    msg: EngineMessage,
    mut state: EngineState,
) -> (EngineState, Vec<EngineIntent>, Vec<EngineEffect>, bool) {
    let dirty = match msg {
        EngineMessage::Select(engine) => {
            let changed = state.engine != engine;
            state.engine = engine;
            changed
        }
    };
    (state, Vec::new(), Vec::new(), dirty)
}
