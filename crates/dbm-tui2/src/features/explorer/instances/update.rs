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
            // Re-load connections for every instance that is expanded after the
            // reload. `set_instances` rebuilds the tree with empty connection
            // lists (they load lazily), so an instance that was already expanded
            // must be refreshed here or its connections would disappear.
            let expanded: Vec<(usize, String)> = state
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.expanded)
                .filter_map(|(i, n)| {
                    n.instance.as_ref().map(|inst| (i, inst.name.clone()))
                })
                .collect();
            for (instance_idx, instance_name) in expanded {
                effects.push(InstancesEffect::LoadConnections {
                    instance_idx,
                    instance_name,
                });
            }
            true
        }
        InstancesMessage::ConnectionsLoaded { instance_idx, connections } => {
            if let Some(node) = state.nodes.get_mut(instance_idx) {
                node.connections = connections;
                node.loaded = true;
                // Activate a restored connection-active once this instance's
                // connections are loaded: if the saved connection is present,
                // highlight it; if this is the target instance but the connection
                // no longer exists, fall back to its parent instance. A restore
                // belonging to a different instance stays pending for its load.
                if let Some((inst_name, conn_name)) = state.restore_active_connection.take() {
                    let this_instance = node.instance.as_ref().is_some_and(|i| i.name == inst_name);
                    if let Some(conn_idx) = node
                        .connections
                        .iter()
                        .position(|c| c.name == conn_name)
                        && this_instance
                    {
                        state.set_active_connection(instance_idx, conn_idx);
                    } else if this_instance {
                        // Target instance loaded but the connection is gone:
                        // degrade to the parent instance.
                        state.set_active_instance(instance_idx);
                    } else {
                        // Not this instance yet; keep the pending restore.
                        state.restore_active_connection = Some((inst_name, conn_name));
                    }
                }
                true
            } else {
                false
            }
        }
        InstancesMessage::RefreshConnections { instance_idx } => {
            // Reload the instance's connections from the store so a change made
            // inside the instance workspace (add/edit/delete) shows up in the
            // left tree immediately (matching the original dbm's
            // `load_instance_connections` on save).
            let instance_name = state
                .nodes
                .get(instance_idx)
                .and_then(|n| n.instance.as_ref())
                .map(|i| i.name.clone());
            if let Some(instance_name) = instance_name {
                effects.push(InstancesEffect::LoadConnections {
                    instance_idx,
                    instance_name,
                });
            }
            false
        }
        InstancesMessage::MoveUp => state.move_up(),
        InstancesMessage::MoveDown => state.move_down(),
        InstancesMessage::JumpTo { row } => state.jump_to(row),
        InstancesMessage::Expand => {
            // Only instance rows expand; connection rows ignore the key. On a
            // fresh expand, lazily load the instance's connections so the
            // subtree renders its connections (matching the original dbm).
            match state.cursor_selection() {
                Some((instance_idx, None)) => {
                    let changed = state.expand();
                    let needs_load = state
                        .nodes
                        .get(instance_idx)
                        .map(|n| !n.loaded)
                        .unwrap_or(false);
                    if changed && needs_load {
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
                    changed
                }
                _ => false,
            }
        }
        InstancesMessage::Collapse => {
            // Only instance rows collapse; connection rows ignore the key.
            match state.cursor_selection() {
                Some((_instance_idx, None)) => state.collapse(),
                _ => false,
            }
        }
        InstancesMessage::ToggleExpandAt { row } => {
            // A mouse marker click toggles the instance at that visible row
            // without moving the cursor. Lazily load connections on a fresh
            // expand (matching `Expand`).
            let (_, inst_idx) = state.visible_row_is_connection(row);
            let changed = state.toggle_expand_at(row);
            if changed
                && inst_idx != usize::MAX
                && state.nodes.get(inst_idx).is_some_and(|n| n.expanded && !n.loaded)
            {
                let instance_name = state.nodes[inst_idx]
                    .instance
                    .as_ref()
                    .map(|i| i.name.clone())
                    .unwrap_or_default();
                effects.push(InstancesEffect::LoadConnections {
                    instance_idx: inst_idx,
                    instance_name,
                });
            }
            changed
        }
        InstancesMessage::ScrollHorizontal { delta, term_width } => {
            // The explorer takes ~20% of terminal width, minus the 2 border
            // columns, matching the view's text viewport.  Clamp so `h_scroll`
            // never grows past the longest content row beyond the viewport.
            // When content fits fully inside the viewport, `max` is 0 and
            // pressing Right is a no-op — matching the original dbm.
            let viewport_w = (term_width as u32 * 20 / 100)
                .saturating_sub(2)
                .max(1) as u16;
            let max_row_w = state.max_row_width();
            let max = max_row_w.saturating_sub(viewport_w);
            state.scroll_horizontal(delta, max)
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
        InstancesMessage::NewConnectionTab => {
            // `n` on a connection row always opens a fresh editor; an instance
            // row is unchanged (falls through to no-op).
            if let Some((instance_idx, Some(conn_idx))) = state.cursor_selection() {
                intents.push(InstancesIntent::NewConnectionWorkspace {
                    instance_idx,
                    connection_idx: conn_idx,
                });
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
    fn connections_loaded_refines_restored_active_connection() {
        use crate::features::explorer::instances::state::ActiveWorkspaceKind;
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a")]);
        // A connection-active was saved but its rows were not loaded yet.
        s.set_active_instance(0);
        s.restore_active_connection = Some(("a".to_string(), "c1".to_string()));

        // Loading the instance's connections refines the active workspace to c1.
        let (s, _i, _e, _d) = update(
            InstancesMessage::ConnectionsLoaded {
                instance_idx: 0,
                connections: vec![dbm_store::InstanceConnection {
                    id: "c1".into(),
                    instance_id: "a".into(),
                    name: "c1".into(),
                    username: "u".into(),
                    database: "d".into(),
                    has_password: false,
                    ssl_mode: String::new(),
                    env_label: None,
                    created_at: "now".into(),
                    updated_at: "now".into(),
                    test_succeeded_at: None,
                    test_failed_at: None,
                }],
            },
            s,
        );
        assert_eq!(
            s.active_workspace,
            Some(ActiveWorkspaceKind::Connection { instance_idx: 0, conn_idx: 0 }),
            "loading connections must refine the active workspace to the connection"
        );
        assert!(s.restore_active_connection.is_none(), "pending restore consumed");
    }

    #[test]
    fn connections_loaded_falls_back_to_instance_when_connection_missing() {
        use crate::features::explorer::instances::state::ActiveWorkspaceKind;
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a")]);
        s.restore_active_connection = Some(("a".to_string(), "gone".to_string()));

        // The target instance loads but the saved connection no longer exists:
        // the active workspace degrades to the parent instance.
        let (s, _i, _e, _d) = update(
            InstancesMessage::ConnectionsLoaded {
                instance_idx: 0,
                connections: vec![dbm_store::InstanceConnection {
                    id: "c1".into(),
                    instance_id: "a".into(),
                    name: "c1".into(),
                    username: "u".into(),
                    database: "d".into(),
                    has_password: false,
                    ssl_mode: String::new(),
                    env_label: None,
                    created_at: "now".into(),
                    updated_at: "now".into(),
                    test_succeeded_at: None,
                    test_failed_at: None,
                }],
            },
            s,
        );
        assert_eq!(
            s.active_workspace,
            Some(ActiveWorkspaceKind::Instance(0)),
            "missing connection must fall back to the parent instance"
        );
        assert!(s.restore_active_connection.is_none(), "pending restore consumed");
    }

    #[test]
    fn loaded_reloads_connections_for_expanded_instances() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a"), inst("b")]);
        s.nodes[0].expanded = true; // A is expanded (its connections load lazily)

        // A full reload (e.g. adding a new instance) rebuilds the tree with
        // empty connection lists; the already-expanded instance must re-load.
        let (s, _i, effects, _d) = update(
            InstancesMessage::Loaded {
                instances: vec![inst("a"), inst("b")],
            },
            s,
        );
        assert!(s.nodes[0].expanded, "A stays expanded after reload");
        assert!(effects.iter().any(|e| matches!(
            e,
            InstancesEffect::LoadConnections { instance_idx: 0, .. }
        )), "expanded instance must re-load connections after a reload");
        // B (not expanded) must not reload.
        assert!(!effects.iter().any(|e| matches!(
            e,
            InstancesEffect::LoadConnections { instance_idx: 1, .. }
        )));
    }

    #[test]
    fn expand_loads_connections_when_expanding() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a")]);
        s.cursor = 0;

        // Fresh expand emits a LoadConnections effect (connections not loaded).
        let (_s, _i, effects, dirty) = update(InstancesMessage::Expand, s);
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
    fn collapse_does_not_reload() {
        let mut s = InstancesState::default();
        s.set_instances(vec![inst("a")]);
        s.nodes[0].expanded = true;
        s.nodes[0].loaded = true;
        s.cursor = 0;

        // Collapsing (expanded -> false) must not emit a load effect.
        let (_s, _i, effects, dirty) = update(InstancesMessage::Collapse, s);
        assert!(effects.is_empty());
        assert!(dirty);
    }

    #[test]
    fn horizontal_scroll_clamps_and_reports_noop() {
        let mut s = InstancesState::default();
        // Instance row " ▸ a_long_enough_name" is wider than the explorer
        // viewport (16 cols), so scrolling is allowed.
        s.set_instances(vec![inst("a_very_long_instance_name_for_scroll_test")]);

        // Scrolling at the left boundary is a no-op (dirty=false).
        let (s2, _i, _e, dirty) =
            update(InstancesMessage::ScrollHorizontal { delta: -1, term_width: 80 }, s);
        assert!(!dirty);
        assert_eq!(s2.h_scroll, 0);

        // Scrolling right moves the offset (content wider than viewport).
        let (s3, _i, _e, dirty) =
            update(InstancesMessage::ScrollHorizontal { delta: 3, term_width: 80 }, s2);
        assert!(dirty);
        assert_eq!(s3.h_scroll, 3);
    }
}
