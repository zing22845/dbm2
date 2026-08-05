//! Discover feature update.

use super::msg::DiscoverMessage;
use super::state::DiscoverState;
use super::intent::DiscoverIntent;
use super::effect::DiscoverEffect;
use super::engine;
use super::results;
use super::targets;

/// Update the discover state by delegating to its child sub-modules. Pure
/// by-value transition: only the touched child state is moved out and back,
/// so no deep clone happens per message.
pub fn update(
    msg: DiscoverMessage,
    mut state: DiscoverState,
) -> (DiscoverState, Vec<DiscoverIntent>, Vec<DiscoverEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    match msg {
        DiscoverMessage::Engine(m) => {
            let engine::msg::EngineMsg::Message(inner) = m;
            let engine_state = std::mem::take(&mut state.engine);
            let (s, i, e) = engine::update::update(inner, engine_state);
            state.engine = s;
            intents.extend(i.into_iter().map(DiscoverIntent::Engine));
            effects.extend(e.into_iter().map(DiscoverEffect::Engine));
        }
        DiscoverMessage::Targets(m) => {
            let targets::msg::TargetsMsg::Message(inner) = m;
            let targets_state = std::mem::take(&mut state.targets);
            let (s, i, e) = targets::update::update(inner, targets_state);
            state.targets = s;
            intents.extend(i.into_iter().map(DiscoverIntent::Targets));
            effects.extend(e.into_iter().map(DiscoverEffect::Targets));
        }
        DiscoverMessage::Results(m) => {
            let results::msg::ResultsMsg::Message(inner) = m;
            let results_state = std::mem::take(&mut state.results);
            let (s, i, e) = results::update::update(inner, results_state);
            state.results = s;
            intents.extend(i.into_iter().map(DiscoverIntent::Results));
            effects.extend(e.into_iter().map(DiscoverEffect::Results));
        }
    }
    (state, intents, effects)
}
