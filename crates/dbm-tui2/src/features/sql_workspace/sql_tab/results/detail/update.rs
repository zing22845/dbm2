//! Results detail sub-module update.

use super::msg::DetailMessage;
use super::state::DetailState;
use super::intent::DetailIntent;
use super::effect::DetailEffect;

pub fn update(
    msg: DetailMessage,
    mut state: DetailState,
) -> (DetailState, Vec<DetailIntent>, Vec<DetailEffect>, bool) {
    let intents = Vec::new();
    let effects = Vec::new();
    let dirty = match msg {
        DetailMessage::Scroll { delta } => {
            let before = state.scroll;
            if delta > 0 {
                state.scroll = state.scroll.saturating_add(delta as usize);
            } else {
                state.scroll = state.scroll.saturating_sub(delta.unsigned_abs() as usize);
            }
            state.scroll != before
        }
    };
    (state, intents, effects, dirty)
}
