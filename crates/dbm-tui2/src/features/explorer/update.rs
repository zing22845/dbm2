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
///
/// The returned `bool` is `dirty`: the OR of the child updates' dirty flags,
/// plus any state this layer changed itself (pane switch, tree rebind).
pub fn update(
    msg: ExplorerMessage,
    mut state: ExplorerState,
) -> (ExplorerState, Vec<ExplorerIntent>, Vec<ExplorerEffect>, bool) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    let dirty = match msg {
        ExplorerMessage::SetPane(pane) => {
            let changed = state.pane != pane;
            state.pane = pane;
            changed
        }
        ExplorerMessage::Instances(m) => {
            let instances::msg::InstancesMsg::Message(inner) = m;
            let s = std::mem::take(&mut state.instances);
            let (s, i, e, d) = instances::update::update(inner, s);
            state.instances = s;
            // Instances -> objects interaction: activating a connection rebinds
            // the object tree to that connection so browsing its databases
            // starts from the selected connection.
            if i.iter().any(|intent| {
                matches!(
                    intent,
                    instances::intent::InstancesIntent::OpenConnectionWorkspace { .. }
                )
            })
                && let Some((instance, connection)) = state.instances.connection_at_cursor() {
                    // Rebinding the tree resets its catalog; kick off the
                    // database fetch so browsing starts loading immediately.
                    state.objects.rebind(instance.clone(), connection.clone());
                    effects.push(ExplorerEffect::Objects(
                        objects::effect::ObjectsEffect::LoadDatabases {
                            instance,
                            connection,
                        },
                    ));
                }
            intents.extend(i.into_iter().map(ExplorerIntent::Instances));
            effects.extend(e.into_iter().map(ExplorerEffect::Instances));
            d
        }
        ExplorerMessage::Objects(m) => {
            let objects::msg::ObjectsMsg::Message(inner) = m;
            let s = std::mem::take(&mut state.objects);
            let (s, i, e, d) = objects::update::update(inner, s);
            state.objects = s;
            intents.extend(i.into_iter().map(ExplorerIntent::Objects));
            effects.extend(e.into_iter().map(ExplorerEffect::Objects));
            d
        }
    };
    (state, intents, effects, dirty)
}
