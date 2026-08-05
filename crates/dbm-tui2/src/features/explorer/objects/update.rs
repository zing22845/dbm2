//! Explorer objects feature update.

use super::msg::ObjectsMessage;
use super::state::ObjectsState;
use super::intent::ObjectsIntent;
use super::effect::ObjectsEffect;

pub fn update(
    _msg: ObjectsMessage,
    _state: ObjectsState,
) -> (ObjectsState, Vec<ObjectsIntent>, Vec<ObjectsEffect>) {
    match _msg {}
}
