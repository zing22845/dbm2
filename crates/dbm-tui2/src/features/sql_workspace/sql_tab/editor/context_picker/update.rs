//! Context picker sub-module update.

use super::msg::ContextPickerMessage;
use super::state::ContextPickerState;
use super::intent::ContextPickerIntent;
use super::effect::ContextPickerEffect;

pub fn update(
    _msg: ContextPickerMessage,
    _state: ContextPickerState,
) -> (ContextPickerState, Vec<ContextPickerIntent>, Vec<ContextPickerEffect>) {
    match _msg {}
}
