//! Explorer feature update.

use super::msg::ExplorerMessage;
use super::state::ExplorerState;
use super::intent::ExplorerIntent;
use super::effect::ExplorerEffect;
use super::instances;
use super::objects;

/// Update the explorer state by delegating to its child sub-modules. Pure
/// by-value transition: only the touched child state is moved out and back,
/// so no deep clone happens per message.
pub fn update(
    msg: ExplorerMessage,
    mut state: ExplorerState,
) -> (ExplorerState, Vec<ExplorerIntent>, Vec<ExplorerEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    match msg {
        ExplorerMessage::Instances(m) => {
            let instances::msg::InstancesMsg::Message(inner) = m;
            let instances_state = std::mem::take(&mut state.instances);
            let (s, i, e) = instances::update::update(inner, instances_state);
            state.instances = s;
            intents.extend(i.into_iter().map(ExplorerIntent::Instances));
            effects.extend(e.into_iter().map(ExplorerEffect::Instances));
        }
        ExplorerMessage::Objects(m) => {
            let objects::msg::ObjectsMsg::Message(inner) = m;
            let objects_state = std::mem::take(&mut state.objects);
            let (s, i, e) = objects::update::update(inner, objects_state);
            state.objects = s;
            intents.extend(i.into_iter().map(ExplorerIntent::Objects));
            effects.extend(e.into_iter().map(ExplorerEffect::Objects));
        }
    }
    (state, intents, effects)
}
