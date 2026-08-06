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
        // `HEADER_BUTTONS` is a compile-time constant. With a single button
        // left/right are no-ops; wrapping activates automatically once the
        // constant grows past one.
        HeaderMessage::MoveLeft | HeaderMessage::MoveRight => {
            wrap_header_button(&mut state.button, HEADER_BUTTONS, matches!(msg, HeaderMessage::MoveLeft));
        }
        HeaderMessage::Activate => {
            intents.push(HeaderIntent::Activate { index: state.button });
        }
    }
    (state, intents, Vec::new())
}

/// Move the header-button cursor one step (left = -1, right = +1), wrapping
/// within `[0, count)` modulo the button count. With a single button this is a
/// no-op. Written as a free function so clippy does not fold the constant
/// `count == 1` into a `modulo_one` diagnostic.
fn wrap_header_button(button: &mut usize, count: usize, left: bool) {
    if count <= 1 {
        return;
    }
    if left {
        *button = (button.saturating_add(count).saturating_sub(1)) % count;
    } else {
        *button = (button.saturating_add(1)) % count;
    }
}
