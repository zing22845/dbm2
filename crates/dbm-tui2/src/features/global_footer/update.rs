//! Global footer feature update.

use super::msg::FooterMessage;
use super::state::FooterState;
use super::intent::FooterIntent;
use super::effect::FooterEffect;

/// Update the global footer state. Pure by-value transition.
///
/// The returned `bool` is `dirty`: whether this update changed the rendered
/// status line. `SetStatus` always changes it; `ClearStatus` is a no-op when
/// the status is already empty.
pub fn update(
    msg: FooterMessage,
    mut state: FooterState,
) -> (FooterState, Vec<FooterIntent>, Vec<FooterEffect>, bool) {
    let dirty = match msg {
        FooterMessage::SetStatus(status) => {
            state.status = status;
            true
        }
        FooterMessage::ClearStatus => {
            let changed = !state.status.is_empty();
            state.status.clear();
            changed
        }
    };
    (state, Vec::new(), Vec::new(), dirty)
}
