//! Session snapshot collection: turn the current [`AppState`] into the
//! on-disk `TuiSessionSnapshot`.
use dbm_store::{
    TUI_SESSION_VERSION, TuiInstanceWorkspaceSnapshot, TuiSessionSnapshot, TuiTabSnapshot,
    TuiTreeSelection, TuiTreeSnapshot,
};

use crate::app::state::AppState;
use crate::app_shell::nav::{ExplorerPane, IwPane};
use crate::app_shell::pane::pane_name;

/// Approximate the SQL tab body height (the track the horizontal splitter is a
/// percentage of) from the cached terminal height. The exact value depends on
/// the live layout (header/footer/border rows), but this is only used to
/// round-trip a persisted percentage to/from rows; the layout re-clamps the
/// materialized rows to `[20%, 80%]` of the true track anyway.
pub(super) fn approx_sql_body_height(state: &AppState) -> u16 {
    state
        .term_height
        .saturating_sub(6) // header + footer + borders
        .max(1)
}
/// Approximate the discover targets/results track height: the discover popup
/// overlays ~75% of the body height (centered), minus the engine selector rows.
/// Only used to round-trip the persisted percentage to/from rows; the layout
/// re-clamps to `[20%, 80%]` of the true track.
pub(super) fn approx_discover_body_height(state: &AppState) -> u16 {
    let body_h = state.term_height.saturating_sub(6).max(1);
    let popup_h = (body_h * 3) / 4;
    popup_h
        .saturating_sub(6) // border (2) + engine selector (~4)
        .max(1)
}
/// Approximate the explorer instances/objects track height: the explorer
/// column's inner height (body minus the outer border). Only used to
/// round-trip the persisted percentage to/from rows; the layout re-clamps to
/// `[20%, 80%]` of the true track.
pub(super) fn approx_explorer_body_height(state: &AppState) -> u16 {
    state
        .term_height
        .saturating_sub(6) // header + footer + borders
        .saturating_sub(2) // explorer outer border
        .max(1)
}
pub(super) fn snapshot_from_app(state: &AppState) -> TuiSessionSnapshot {
    let track_h = approx_sql_body_height(state);
    let tabs = state
        .sql
        .sql_tab
        .tabs
        .iter()
        .map(|tab| {
            let session = &tab.session;
            let sql = crate::common::editor::editor_text(&tab.editor.editor);
            TuiTabSnapshot {
                instance: session.instance.clone().unwrap_or_default(),
                connection: session.connection.clone().unwrap_or_default(),
                sequence: session.sequence as u32,
                sql,
                // Persist the horizontal split as a percentage (stable across
                // terminals); the running state keeps it in absolute rows.
                split_ratio: tab.splitter.editor_top_pct(track_h),
                history_pane_width: tab.splitter.history_pane_width,
                detail_pane_width: tab.history.splitter.detail_pane_width,
                database: session.database.clone().unwrap_or_default(),
                schema: session.schema.clone().unwrap_or_default(),
                complete_table_names: tab.complete_table_names,
            }
        })
        .collect();

    TuiSessionSnapshot {
        version: TUI_SESSION_VERSION,
        focus: pane_name(state.focus).to_string(),
        // `tree_width` doubles as the Explorer / workspace splitter width (the
        // Explorer pane is the tree); persisted in absolute columns.
        tree_width: state.splitter.explorer_pane_width,
        tree: tree_snapshot(&state.explorer),
        tabs,
        active_tab: state.sql.sql_tab.active_tab,
        instance_workspace: iw_snapshot(state),
        // Persist the discover targets/results split as a percentage (stable
        // across terminals); the running state keeps it in absolute rows.
        discover_targets_ratio: state
            .discover
            .splitter
            .targets_height_pct(approx_discover_body_height(state)),
        // Persist the explorer instances/objects split as a percentage (stable
        // across terminals); the running state keeps it in absolute rows.
        explorer_split_ratio: state
            .explorer
            .splitter
            .instances_height_pct(approx_explorer_body_height(state)),
        explorer_pane: match state.explorer.pane {
            ExplorerPane::Instances => "instances",
            ExplorerPane::Objects => "objects",
        }
        .into(),
    }
}
/// Persist the explorer tree's expansion and cursor state. Instances are
/// remembered by name; the objects tree by its expansion keys.
pub(super) fn tree_snapshot(
    explorer: &crate::features::explorer::state::ExplorerState,
) -> TuiTreeSnapshot {
    // Expanded instance names (expansion lives on each node's `expanded` flag).
    let expanded_instances: Vec<String> = explorer
        .instances
        .nodes
        .iter()
        .filter(|n| n.expanded)
        .filter_map(|n| n.instance.as_ref())
        .map(|i| i.name.clone())
        .collect();

    // Cursor: resolve the highlighted row to an instance or connection name.
    let cursor = explorer.instances.cursor_selection().and_then(|(i, conn)| {
        let instance = explorer.instances.instance_name(i);
        if instance.is_empty() {
            return None;
        }
        Some(match conn {
            Some(ci) => {
                let connection = explorer
                    .instances
                    .nodes
                    .get(i)
                    .and_then(|n| n.connections.get(ci))
                    .map(|c| c.name.clone())
                    .unwrap_or_default();
                if connection.is_empty() {
                    return None;
                }
                TuiTreeSelection::Connection {
                    instance,
                    connection,
                }
            }
            None => TuiTreeSelection::Instance { instance },
        })
    });

    // Sort for a deterministic order (the underlying storage is a HashSet, whose
    // iteration order is not stable) so the serialized session and tests are
    // reproducible.
    let mut expanded_objects: Vec<String> = explorer.objects.expanded.iter().cloned().collect();
    expanded_objects.sort();

    // The active workspace node (instance or connection), so restarting restores
    // the active-row highlight and the workspace shown.
    let active_workspace = explorer.instances.active_workspace.and_then(|aw| {
        use crate::features::explorer::instances::state::ActiveWorkspaceKind;
        match aw {
            ActiveWorkspaceKind::Instance(instance_idx) => {
                let instance = explorer.instances.instance_name(instance_idx);
                if instance.is_empty() {
                    None
                } else {
                    Some(TuiTreeSelection::Instance { instance })
                }
            }
            ActiveWorkspaceKind::Connection {
                instance_idx,
                conn_idx,
            } => {
                let instance = explorer.instances.instance_name(instance_idx);
                let connection = explorer
                    .instances
                    .nodes
                    .get(instance_idx)
                    .and_then(|n| n.connections.get(conn_idx))
                    .map(|c| c.name.clone())
                    .unwrap_or_default();
                if instance.is_empty() || connection.is_empty() {
                    None
                } else {
                    Some(TuiTreeSelection::Connection {
                        instance,
                        connection,
                    })
                }
            }
        }
    });

    TuiTreeSnapshot {
        expanded_instances,
        cursor,
        active_workspace,
        expanded_objects,
        objects_bound_instance: explorer.objects.bound_instance.clone(),
        objects_bound_connection: explorer.objects.bound_connection.clone(),
        objects_active_db: explorer.objects.active_db.clone(),
        objects_active_schema: explorer.objects.active_schema.clone(),
    }
}
/// Persist the instance-workspace sub-pane focus and connections cursor.
pub(super) fn iw_snapshot(state: &AppState) -> Option<TuiInstanceWorkspaceSnapshot> {
    if !state.instance_workspace_open() {
        return None;
    }
    Some(TuiInstanceWorkspaceSnapshot {
        section: match state.iw.pane {
            IwPane::Overview => "overview",
            IwPane::Connections => "connections",
        }
        .to_string(),
        connections_cursor: state.iw.connections.cursor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> AppState {
        AppState::default()
    }

    fn managed_instance(name: &str) -> dbm_store::ManagedInstance {
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

    fn connection(name: &str) -> dbm_store::InstanceConnection {
        dbm_store::InstanceConnection {
            id: format!("c-{name}"),
            instance_id: "id".to_string(),
            name: name.to_string(),
            username: "postgres".to_string(),
            database: "postgres".to_string(),
            has_password: false,
            ssl_mode: String::new(),
            env_label: None,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
            test_succeeded_at: None,
            test_failed_at: None,
        }
    }

    #[test]
    fn snapshot_collects_tree_and_iw_state() {
        let mut state = sample_state();
        // Instances tree: two instances, first expanded, cursor on "app".
        state
            .explorer
            .instances
            .set_instances(vec![managed_instance("local"), managed_instance("remote")]);
        state.explorer.instances.nodes[0].expanded = true;
        state.explorer.instances.nodes[0].connections = vec![connection("app")];
        state.explorer.instances.nodes[0].loaded = true;
        // Cursor on the "app" connection row (row 1).
        state.explorer.instances.cursor = 1;
        // Instance workspace is active (drives `instance_workspace_open()`).
        state.explorer.instances.set_active_instance(0);
        // Objects tree expansion keys + active schema.
        state.explorer.objects.expanded.insert("mydb".to_string());
        state
            .explorer
            .objects
            .expanded
            .insert("mydb\tpublic".to_string());
        state.explorer.objects.active_db = Some("mydb".into());
        state.explorer.objects.active_schema = Some("public".into());
        // Explorer focuses the objects sub-pane.
        state.explorer.pane = ExplorerPane::Objects;
        // Instance workspace on the connections pane with a cursor.
        state.iw.instance_name = "local".into();
        state.iw.pane = IwPane::Connections;
        state.iw.connections.connections = vec![connection("app"), connection("analytics")];
        state.iw.connections.cursor = 1;

        let snap = snapshot_from_app(&state);
        assert_eq!(snap.tree.expanded_instances, vec!["local".to_string()]);
        assert_eq!(
            snap.tree.cursor,
            Some(TuiTreeSelection::Connection {
                instance: "local".into(),
                connection: "app".into(),
            })
        );
        assert_eq!(
            snap.tree.expanded_objects,
            vec!["mydb".to_string(), "mydb\tpublic".to_string()]
        );
        // The active workspace (instance 0) is persisted.
        assert_eq!(
            snap.tree.active_workspace,
            Some(TuiTreeSelection::Instance {
                instance: "local".into()
            })
        );
        // The objects tree's active schema is persisted too.
        assert_eq!(snap.tree.objects_active_db.as_deref(), Some("mydb"));
        assert_eq!(snap.tree.objects_active_schema.as_deref(), Some("public"));
        assert_eq!(snap.explorer_pane, "objects");
        let iw = snap.instance_workspace.expect("iw snapshot present");
        assert_eq!(iw.section, "connections");
        assert_eq!(iw.connections_cursor, 1);
    }
}
