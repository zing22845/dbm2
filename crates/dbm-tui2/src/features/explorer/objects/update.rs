//! Explorer objects feature update.

use super::msg::ObjectsMessage;
use super::state::ObjectsState;
use super::intent::ObjectsIntent;
use super::effect::ObjectsEffect;

/// Update the objects tree state. Pure by-value transition.
pub fn update(
    msg: ObjectsMessage,
    mut state: ObjectsState,
) -> (ObjectsState, Vec<ObjectsIntent>, Vec<ObjectsEffect>) {
    let mut intents = Vec::new();
    match msg {
        ObjectsMessage::MoveUp => state.move_up(),
        ObjectsMessage::MoveDown => state.move_down(),
        ObjectsMessage::ToggleExpand => state.toggle_expand(),
        ObjectsMessage::Select => {
            // Selecting an object (e.g. a table) notifies the SQL workspace to
            // open it. This is a cross-feature intent, consumed at the shell
            // layer once the sql workspace is migrated.
            if let Some(target) = state.selected_target() {
                intents.push(ObjectsIntent::OpenObject { target });
            }
        }
        ObjectsMessage::Bind { instance, connection } => state.rebind(instance, connection),
    }
    (state, intents, Vec::new())
}
