//! Explorer instances feature update.

use super::msg::InstancesMessage;
use super::state::InstancesState;
use super::intent::InstancesIntent;
use super::effect::InstancesEffect;

/// Update the instances (connection tree) state. Pure by-value transition.
///
/// The returned `bool` is `dirty`: whether the rendered tree changed.
/// Navigation reports `false` when clamped at a boundary; `Select` and `Load`
/// do not change the tree themselves (the shell reacts to their intents).
pub fn update(
    msg: InstancesMessage,
    mut state: InstancesState,
) -> (InstancesState, Vec<InstancesIntent>, Vec<InstancesEffect>, bool) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    let dirty = match msg {
        InstancesMessage::Load => {
            effects.push(InstancesEffect::LoadInstances);
            false
        }
        InstancesMessage::Loaded { instances } => {
            state.set_instances(instances);
            true
        }
        InstancesMessage::ConnectionsLoaded { instance_idx, connections } => {
            if let Some(node) = state.nodes.get_mut(instance_idx) {
                node.connections = connections;
                node.loaded = true;
                true
            } else {
                false
            }
        }
        InstancesMessage::MoveUp => state.move_up(),
        InstancesMessage::MoveDown => state.move_down(),
        InstancesMessage::ToggleExpand => {
            // Only instance rows expand; connection rows ignore the key. On
            // expand, lazily load the instance's connections so the subtree
            // renders its connections (matching the original dbm).
            match state.cursor_selection() {
                Some((instance_idx, None)) => {
                    let was_expanded = state.toggle_expand();
                    let now_expanded = state
                        .nodes
                        .get(instance_idx)
                        .map(|n| n.expanded)
                        .unwrap_or(false);
                    let needs_load = state
                        .nodes
                        .get(instance_idx)
                        .map(|n| !n.loaded)
                        .unwrap_or(false);
                    if was_expanded && now_expanded && needs_load {
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
                    was_expanded
                }
                _ => false,
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
            false
        }
    };
    (state, intents, effects, dirty)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(name: &str) -> dbm_store::ManagedInstance {
        dbm_store::ManagedInstance {
            id: format!("id-{name}"),
            fingerprint: format!("fp-{name}"),
            name: name.to_string(),
            engine: dbm_core::Engine::Postgres,
            host: "127.0.0.1".to_string(),
            port: 5432,
            socket_path: None,
            data_dir: None,
            env_label: None,
            registered_at: "now".to_string(),
            version_full: None,
            version_short: None,
            version_checked_at: None,
            lifecycle_status: None,
            lifecycle_checked_at: None,
            lifecycle_detail: None,
        }
    }

    #[test]
    fn toggle_expand_loads_connections_when_expanding() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a")]);
        s.cursor = 0;

        // First expand emits a LoadConnections effect (connections not loaded).
        let (_s, _i, effects, dirty) = update(InstancesMessage::ToggleExpand, s);
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            InstancesEffect::LoadConnections { instance_idx, instance_name } => {
                assert_eq!(*instance_idx, 0);
                assert_eq!(instance_name, "a");
            }
            other => panic!("expected LoadConnections, got {other:?}"),
        }
        assert!(dirty);
    }

    #[test]
    fn toggle_collapse_does_not_reload() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a")]);
        s.nodes[0].expanded = true;
        s.nodes[0].loaded = true;
        s.cursor = 0;

        // Collapsing (expanded -> false) must not emit a load effect.
        let (_s, _i, effects, _dirty) = update(InstancesMessage::ToggleExpand, s);
        assert!(effects.is_empty());
    }
}
