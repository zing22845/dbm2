//! Instance workspace feature update.

use super::msg::IwMessage;
use super::state::IwState;
use super::intent::IwIntent;
use super::effect::IwEffect;
use super::connections;
use super::overview;

/// Update the instance workspace state by delegating to its child sub-modules.
/// Pure by-value transition: only the touched child state is moved out and
/// back, so no deep clone happens per message.
pub fn update(
    msg: IwMessage,
    mut state: IwState,
) -> (IwState, Vec<IwIntent>, Vec<IwEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    match msg {
        IwMessage::Overview(m) => {
            let overview::msg::OverviewMsg::Message(inner) = m;
            let overview_state = std::mem::take(&mut state.overview);
            let (s, i, e) = overview::update::update(inner, overview_state);
            state.overview = s;
            intents.extend(i.into_iter().map(IwIntent::Overview));
            effects.extend(e.into_iter().map(IwEffect::Overview));
        }
        IwMessage::Connections(m) => {
            let connections::msg::ConnectionsMsg::Message(inner) = m;
            let connections_state = std::mem::take(&mut state.connections);
            let (s, i, e) = connections::update::update(inner, connections_state);
            state.connections = s;
            intents.extend(i.into_iter().map(IwIntent::Connections));
            effects.extend(e.into_iter().map(IwEffect::Connections));
        }
    }
    (state, intents, effects)
}
