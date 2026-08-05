//! Explorer feature update.

use super::msg::ExplorerMessage;
use super::state::ExplorerState;
use super::intent::ExplorerIntent;
use super::effect::ExplorerEffect;
use super::instances;
use super::objects;

/// Update the explorer state. Pure by-value transition: pane navigation is
/// handled here; child messages are forwarded to the matching sub-module
/// (moved out, updated, moved back).
pub fn update(
    msg: ExplorerMessage,
    mut state: ExplorerState,
) -> (ExplorerState, Vec<ExplorerIntent>, Vec<ExplorerEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    match msg {
        ExplorerMessage::SetPane(pane) => state.pane = pane,
        ExplorerMessage::Instances(m) => {
            let instances::msg::InstancesMsg::Message(inner) = m;
            let s = std::mem::take(&mut state.instances);
            let (s, i, e) = instances::update::update(inner, s);
            state.instances = s;
            // Instances -> objects interaction: activating a connection rebinds
            // the object tree to that connection so browsing its databases
            // starts from the selected connection.
            if i.iter().any(|intent| {
                matches!(
                    intent,
                    instances::intent::InstancesIntent::OpenConnectionWorkspace { .. }
                )
            }) {
                if let Some((instance, connection)) = state.instances.connection_at_cursor() {
                    state.objects.rebind(instance, connection);
                }
            }
            intents.extend(i.into_iter().map(ExplorerIntent::Instances));
            effects.extend(e.into_iter().map(ExplorerEffect::Instances));
        }
        ExplorerMessage::Objects(m) => {
            let objects::msg::ObjectsMsg::Message(inner) = m;
            let s = std::mem::take(&mut state.objects);
            let (s, i, e) = objects::update::update(inner, s);
            state.objects = s;
            intents.extend(i.into_iter().map(ExplorerIntent::Objects));
            effects.extend(e.into_iter().map(ExplorerEffect::Objects));
        }
    }
    (state, intents, effects)
}
