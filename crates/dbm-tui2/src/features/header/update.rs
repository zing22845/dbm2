//! Header feature update.

use super::msg::HeaderMessage;
use super::state::{HeaderState, HEADER_BUTTONS};
use super::intent::HeaderIntent;
use super::effect::HeaderEffect;

/// Update the header state. Pure by-value transition.
pub fn update(
    msg: HeaderMessage,
    mut state: HeaderState,
) -> (HeaderState, Vec<HeaderIntent>, Vec<HeaderEffect>) {
    let mut intents = Vec::new();
    match msg {
        HeaderMessage::MoveLeft => {
            state.button = (state.button + HEADER_BUTTONS - 1) % HEADER_BUTTONS;
        }
        HeaderMessage::MoveRight => {
            state.button = (state.button + 1) % HEADER_BUTTONS;
        }
        HeaderMessage::Activate => {
            intents.push(HeaderIntent::Activate { index: state.button });
        }
    }
    (state, intents, Vec::new())
}
