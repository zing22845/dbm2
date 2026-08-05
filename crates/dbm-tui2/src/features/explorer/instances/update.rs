//! Explorer instances feature update.

use super::msg::InstancesMessage;
use super::state::InstancesState;
use super::intent::InstancesIntent;
use super::effect::InstancesEffect;

pub fn update(
    _msg: InstancesMessage,
    _state: InstancesState,
) -> (InstancesState, Vec<InstancesIntent>, Vec<InstancesEffect>) {
    match _msg {}
}
