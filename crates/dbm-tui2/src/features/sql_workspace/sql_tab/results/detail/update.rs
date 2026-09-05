//! Results detail sub-module update.

use super::super::detail_edit::detail_draft_dirty;
use super::effect::DetailEffect;
use super::intent::DetailIntent;
use super::msg::DetailMessage;
use super::state::DetailState;

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
        DetailMessage::SetDraft { text } => {
            state.draft = text.clone();
            state.dirty = detail_draft_dirty(&text, &state.baseline);
            true
        }
        DetailMessage::LoadCell { value } => {
            state.load_cell(&value);
            true
        }
        DetailMessage::ClearDraft => {
            state.clear_draft();
            true
        }
    };
    (state, intents, effects, dirty)
}
