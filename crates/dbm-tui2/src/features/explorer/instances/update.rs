//! Explorer instances feature update.

use super::msg::InstancesMessage;
use super::state::InstancesState;
use super::intent::InstancesIntent;
use super::effect::InstancesEffect;

/// Update the instances (connection tree) state. Pure by-value transition.
pub fn update(
    msg: InstancesMessage,
    mut state: InstancesState,
) -> (InstancesState, Vec<InstancesIntent>, Vec<InstancesEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    match msg {
        InstancesMessage::Load => {
            effects.push(InstancesEffect::LoadInstances);
        }
        InstancesMessage::Loaded { instances } => {
            state.set_instances(instances);
        }
        InstancesMessage::ConnectionsLoaded { instance_idx, connections } => {
            if let Some(node) = state.nodes.get_mut(instance_idx) {
                node.connections = connections;
                node.loaded = true;
            }
        }
        InstancesMessage::MoveUp => state.move_up(),
        InstancesMessage::MoveDown => state.move_down(),
        InstancesMessage::ToggleExpand => {
            // Only instance rows expand; connection rows ignore the key.
            if let Some((_, None)) = state.cursor_selection() {
                state.toggle_expand();
            }
        }
        InstancesMessage::Select => {
            match state.cursor_selection() {
                Some((instance_idx, None)) => {
                    intents.push(InstancesIntent::OpenInstanceWorkspace { instance_idx });
                }
                Some((instance_idx, Some(conn_idx))) => {
                    // Lazily load the instance's connections so the connection
                    // workspace has fresh data, and notify the shell to open it.
                    let needs_load = state
                        .nodes
                        .get(instance_idx)
                        .map(|n| !n.loaded)
                        .unwrap_or(false);
                    if needs_load {
                        let instance_name = state.nodes[instance_idx]
                            .instance
                            .as_ref()
                            .map(|i| i.name.clone())
                            .unwrap_or_default();
                        effects.push(InstancesEffect::LoadConnections {
                            instance_idx,
                            instance_name,
                        });
                    }
                    intents.push(InstancesIntent::OpenConnectionWorkspace {
                        instance_idx,
                        connection_idx: conn_idx,
                    });
                }
                None => {}
            }
        }
    }
    (state, intents, effects)
}
