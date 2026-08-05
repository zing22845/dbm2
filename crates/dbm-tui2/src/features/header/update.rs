//! Header feature update.

use super::msg::HeaderMessage;
use super::state::HeaderState;
use super::intent::HeaderIntent;
use super::effect::HeaderEffect;

/// Update the header state in response to a message. Returns the new state
/// plus any side-channel intents and effects.
pub fn update(
    _msg: HeaderMessage,
    _state: &mut HeaderState,
) -> (HeaderState, Vec<HeaderIntent>, Vec<HeaderEffect>) {
    // Skeleton: no business logic yet. The inner message enum is uninhabited,
    // so this match is exhaustive without arms.
    match _msg {}
}
