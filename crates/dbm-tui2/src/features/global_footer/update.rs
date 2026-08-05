//! Global footer feature update.

use super::msg::FooterMessage;
use super::state::FooterState;
use super::intent::FooterIntent;
use super::effect::FooterEffect;

/// Update the global footer state. Pure by-value transition.
pub fn update(
    msg: FooterMessage,
    mut state: FooterState,
) -> (FooterState, Vec<FooterIntent>, Vec<FooterEffect>) {
    match msg {
        FooterMessage::SetStatus(status) => state.status = status,
        FooterMessage::ClearStatus => state.status.clear(),
    }
    (state, Vec::new(), Vec::new())
}
