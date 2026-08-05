//! Instance connections feature update.

use super::msg::ConnectionsMessage;
use super::state::ConnectionsState;
use super::intent::ConnectionsIntent;
use super::effect::ConnectionsEffect;

pub fn update(
    _msg: ConnectionsMessage,
    _state: ConnectionsState,
) -> (ConnectionsState, Vec<ConnectionsIntent>, Vec<ConnectionsEffect>) {
    match _msg {}
}
