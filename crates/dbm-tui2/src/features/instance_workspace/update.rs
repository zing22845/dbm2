//! Instance workspace feature update.

use super::msg::IwMessage;
use super::state::IwState;
use super::intent::IwIntent;
use super::effect::IwEffect;
use super::connections;
use super::overview;

/// Update the instance workspace state. Pure by-value transition: opening an
/// instance triggers the overview + connections loads; child messages are
/// forwarded to the matching sub-module (moved out, updated, moved back).
pub fn update(
    msg: IwMessage,
    mut state: IwState,
) -> (IwState, Vec<IwIntent>, Vec<IwEffect>, bool) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    let dirty = match msg {
        IwMessage::OpenInstance { instance_name } => {
            let changed = state.instance_name != instance_name;
            state.instance_name = instance_name.clone();
            // Load the overview and connections for the freshly opened instance
            // by dispatching child Load messages.
            let (ov, _oi, oe, od) = overview::update::update(
                overview::msg::OverviewMessage::Load {
                    instance_name: instance_name.clone(),
                },
                std::mem::take(&mut state.overview),
            );
            state.overview = ov;
            effects.extend(oe.into_iter().map(IwEffect::Overview));
            let (cn, _ci, ce, cd) = connections::update::update(
                connections::msg::ConnectionsMessage::Load {
                    instance_name: instance_name.clone(),
                },
                std::mem::take(&mut state.connections),
            );
            state.connections = cn;
            effects.extend(ce.into_iter().map(IwEffect::Connections));
            changed || od || cd
        }
        IwMessage::Overview(m) => {
            let overview::msg::OverviewMsg::Message(inner) = m;
            let s = std::mem::take(&mut state.overview);
            let (s, i, e, d) = overview::update::update(inner, s);
            state.overview = s;
            intents.extend(i.into_iter().map(IwIntent::Overview));
            effects.extend(e.into_iter().map(IwEffect::Overview));
            d
        }
        IwMessage::Connections(m) => {
            let connections::msg::ConnectionsMsg::Message(inner) = m;
            let s = std::mem::take(&mut state.connections);
            let (s, i, e, d) = connections::update::update(inner, s);
            state.connections = s;
            intents.extend(i.into_iter().map(IwIntent::Connections));
            effects.extend(e.into_iter().map(IwEffect::Connections));
            d
        }
    };
    (state, intents, effects, dirty)
}
