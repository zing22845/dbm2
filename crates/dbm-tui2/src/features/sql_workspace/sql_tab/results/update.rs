//! Results feature update.

use super::msg::ResultsMessage;
use super::state::ResultsState;
use super::intent::ResultsIntent;
use super::effect::ResultsEffect;
use super::detail;

pub fn update(
    msg: ResultsMessage,
    mut state: ResultsState,
) -> (ResultsState, Vec<ResultsIntent>, Vec<ResultsEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    match msg {
        ResultsMessage::Detail(m) => {
            let detail::msg::DetailMsg::Message(inner) = m;
            let detail_state = std::mem::take(&mut state.detail);
            let (s, i, e) = detail::update::update(inner, detail_state);
            state.detail = s;
            intents.extend(i.into_iter().map(ResultsIntent::Detail));
            effects.extend(e.into_iter().map(ResultsEffect::Detail));
        }
    }
    (state, intents, effects)
}
