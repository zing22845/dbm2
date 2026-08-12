//! TUI session persistence: save/restore the open SQL tabs and their context
//! across app restarts.
//!
//! The on-disk schema lives in `dbm_store` (`TuiSessionSnapshot`, stored as
//! JSON under `tui_session`). This module adapts the current `AppState` to that
//! schema. Fields the shell does not yet track (split ratios, tree width, etc.)
//! are persisted as defaults and round-trip harmlessly; the schema is additive
//! so older/newer snapshots remain compatible as the shell grows.

use dbm_store::{
    TUI_SESSION_VERSION, TuiInstanceWorkspaceSnapshot, TuiSessionSnapshot, TuiTabSnapshot,
    TuiTreeSelection, TuiTreeSnapshot, load_tui_session, save_tui_session,
};

use crate::app::action::Action;
use crate::app::state::AppState;
use crate::app_shell::effect::ErasedEffect;
use crate::app_shell::nav::{ExplorerPane, IwPane};
use crate::app_shell::pane::{Pane, pane_from_name, pane_name};

/// Restore a previously persisted session into `state`. Returns the
/// `LoadConnections` effects needed to fetch the subtrees of instances that
/// were restored expanded (their connections are loaded lazily). Returns an
/// empty vec when no compatible snapshot exists (fresh start).
pub fn restore_session(state: &mut AppState) -> anyhow::Result<Vec<Box<dyn ErasedEffect<Action>>>> {
    let Some(snapshot) = load_tui_session()? else {
        return Ok(Vec::new());
    };
    Ok(apply_snapshot(state, &snapshot))
}

/// Persist the current session. Best-effort: a failure to write is surfaced to
/// the caller (which may log or ignore it at exit).
pub fn persist_session(state: &AppState) -> anyhow::Result<()> {
    save_tui_session(&snapshot_from_app(state))
        .map_err(|e| anyhow::anyhow!("failed to save TUI session: {e}"))
}

fn snapshot_from_app(state: &AppState) -> TuiSessionSnapshot {
    let tabs = state
        .sql
        .sql_tab
        .tabs
        .iter()
        .enumerate()
        .map(|(_idx, tab)| {
            let session = &tab.session;
            let sql = crate::common::editor::editor_text(&tab.editor.editor);
            TuiTabSnapshot {
                instance: session.instance.clone().unwrap_or_default(),
                connection: session.connection.clone().unwrap_or_default(),
                sequence: session.sequence as u32,
                sql,
                split_ratio: tab.split_ratio,
                history_pane_width: tab.history_pane_width,
                detail_pane_width: 32,
                database: session.database.clone().unwrap_or_default(),
                schema: session.schema.clone().unwrap_or_default(),
                complete_table_names: false,
            }
        })
        .collect();

    TuiSessionSnapshot {
        version: TUI_SESSION_VERSION,
        focus: pane_name(state.focus).to_string(),
        tree_width: 20,
        tree: tree_snapshot(&state.explorer),
        tabs,
        active_tab: Some(state.sql.sql_tab.active_tab),
        instance_workspace: iw_snapshot(state),
        discover_targets_ratio: 35,
        explorer_split_ratio: 20,
        explorer_pane: match state.explorer.pane {
            ExplorerPane::Instances => "instances",
            ExplorerPane::Objects => "objects",
        }
        .into(),
    }
}

/// Persist the explorer tree's expansion and cursor state. Instances are
/// remembered by name; the objects tree by its expansion keys.
fn tree_snapshot(explorer: &crate::features::explorer::state::ExplorerState) -> TuiTreeSnapshot {
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
                TuiTreeSelection::Connection { instance, connection }
            }
            None => TuiTreeSelection::Instance { instance },
        })
    });

    // Sort for a deterministic order (the underlying storage is a HashSet, whose
    // iteration order is not stable) so the serialized session and tests are
    // reproducible.
    let mut expanded_objects: Vec<String> =
        explorer.objects.expanded.iter().cloned().collect();
    expanded_objects.sort();

    TuiTreeSnapshot {
        expanded_instances,
        cursor,
        active_workspace: None,
        expanded_objects,
        objects_bound_instance: explorer.objects.bound_instance.clone(),
        objects_bound_connection: explorer.objects.bound_connection.clone(),
    }
}

/// Persist the instance-workspace sub-pane focus and connections cursor.
fn iw_snapshot(state: &AppState) -> Option<TuiInstanceWorkspaceSnapshot> {
    if state.iw.instance_name.is_empty() {
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

fn apply_snapshot(state: &mut AppState, snapshot: &TuiSessionSnapshot) -> Vec<Box<dyn ErasedEffect<Action>>> {
    use crate::features::sql_workspace::sql_tab::editor::state::EditorState;

    // Rebuild the tab list from the snapshot.
    let tabs: Vec<crate::features::sql_workspace::sql_tab::state::SqlTab> = snapshot
        .tabs
        .iter()
        .map(|t| crate::features::sql_workspace::sql_tab::state::SqlTab {
            session: crate::features::sql_workspace::sql_tab::session::TabSession {
                id: t.sequence as usize,
                sequence: t.sequence as usize,
                instance: non_empty(t.instance.clone()),
                connection: non_empty(t.connection.clone()),
                connection_id: None,
                database: non_empty(t.database.clone()),
                schema: non_empty(t.schema.clone()),
            },
            focus: crate::features::sql_workspace::sql_tab::state::SqlFocus::default(),
            split_ratio: t.split_ratio,
            history_pane_width: t.history_pane_width,
            editor: EditorState::with_sql(&t.sql),
            results: crate::features::sql_workspace::sql_tab::results::state::ResultsState::new(),
            history: crate::features::sql_workspace::sql_tab::history::state::HistoryState::default(),
        })
        .collect();

    // Replace the default single tab with the restored set. When there are no
    // persisted tabs, keep the default empty tab.
    if !tabs.is_empty() {
        let active = snapshot
            .active_tab
            .filter(|&i| i < tabs.len())
            .unwrap_or(0);
        state.sql.sql_tab.tabs = tabs;
        state.sql.sql_tab.active_tab = active;
    }

    state.focus = pane_from_name(&snapshot.focus).unwrap_or(Pane::Header);

    // Restore the explorer sub-pane focus (instances / objects).
    state.explorer.pane = match snapshot.explorer_pane.as_str() {
        "objects" => ExplorerPane::Objects,
        _ => ExplorerPane::Instances,
    };

    // Restore instance expansion + cursor by name. Expansion is applied to the
    // freshly-loaded tree nodes; the cursor resolves to the owning instance row
    // (connections load lazily, so a connection cursor focuses its instance).
    let mut restore_effects: Vec<Box<dyn ErasedEffect<Action>>> = Vec::new();
    apply_instances_tree(state, snapshot, &mut restore_effects);
    apply_objects_expansion(state, snapshot);

    // Restore the instance-workspace sub-pane and connections cursor.
    if let Some(iw) = &snapshot.instance_workspace {
        state.iw.pane = match iw.section.as_str() {
            "connections" => IwPane::Connections,
            _ => IwPane::Overview,
        };
        let max = state.iw.connections.connections.len().saturating_sub(1);
        state.iw.connections.cursor = iw.connections_cursor.min(max);
    }

    restore_effects
}

/// Restore the instances tree's expansion set and cursor from the snapshot.
/// Returns `LoadConnections` effects for every restored-expanded instance whose
/// connections are not yet loaded, so the lazily-loaded subtrees are fetched
/// right after startup (matching the interactive `Expand` behavior).
fn apply_instances_tree(
    state: &mut AppState,
    snapshot: &TuiSessionSnapshot,
    effects: &mut Vec<Box<dyn ErasedEffect<Action>>>,
) {
    let expanded: std::collections::HashSet<String> =
        snapshot.tree.expanded_instances.iter().cloned().collect();
    for (idx, node) in state.explorer.instances.nodes.iter_mut().enumerate() {
        let name = node.instance.as_ref().map(|i| i.name.clone()).unwrap_or_default();
        node.expanded = !name.is_empty() && expanded.contains(&name);
        if node.expanded && !node.loaded {
            let effect = crate::features::explorer::effect::ExplorerEffect::Instances(
                crate::features::explorer::instances::effect::InstancesEffect::LoadConnections {
                    instance_idx: idx,
                    instance_name: name,
                },
            );
            effects.push(Box::new(effect) as Box<dyn ErasedEffect<Action>>);
        }
    }

    let Some(sel) = &snapshot.tree.cursor else {
        return;
    };
    let instance_name = match sel {
        TuiTreeSelection::Instance { instance } => instance,
        TuiTreeSelection::Connection { instance, .. } => instance,
    };
    // Locate the instance row and move the cursor onto it.
    let mut row = 0usize;
    for node in &state.explorer.instances.nodes {
        if let Some(inst) = &node.instance {
            if &inst.name == instance_name {
                state.explorer.instances.cursor = row;
                state.explorer.instances.scroll = row;
                return;
            }
        }
        row += 1;
        if node.expanded {
            row += node.connections.len();
        }
    }
}

/// Stage the objects tree's expansion keys for restore. The tree is usually
/// unbound right after startup, so the keys are parked until it is next bound
/// to the saved connection (`ObjectsState::rebind`), at which point they are
/// re-applied and the catalog is re-fetched.
fn apply_objects_expansion(state: &mut AppState, snapshot: &TuiSessionSnapshot) {
    if snapshot.tree.expanded_objects.is_empty() {
        return;
    }
    state.explorer.objects.restore_expanded = snapshot.tree.expanded_objects.clone();
    state.explorer.objects.restore_bound_connection =
        snapshot.tree.objects_bound_connection.clone();
}

fn non_empty(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> AppState {
        AppState::default()
    }

    #[test]
    fn snapshot_round_trips_default_state() {
        let state = sample_state();
        let snap = snapshot_from_app(&state);
        assert_eq!(snap.version, TUI_SESSION_VERSION);
        assert_eq!(snap.focus, "header");
        // Default app opens no tabs (a tab is opened when a connection is
        // selected in the tree).
        assert!(snap.tabs.is_empty());
    }

    #[test]
    fn restore_replaces_tabs_from_snapshot() {
        let snap = TuiSessionSnapshot {
            version: TUI_SESSION_VERSION,
            focus: "sql_workspace".into(),
            tree_width: 20,
            tree: TuiTreeSnapshot {
                expanded_instances: Vec::new(),
                cursor: None,
                active_workspace: None,
                expanded_objects: Vec::new(),
                objects_bound_instance: String::new(),
                objects_bound_connection: String::new(),
            },
            tabs: vec![
                TuiTabSnapshot {
                    instance: "local".into(),
                    connection: "app".into(),
                    sequence: 0,
                    sql: "select 1".into(),
                    split_ratio: 45,
                    history_pane_width: 24,
                    detail_pane_width: 32,
                    database: "mydb".into(),
                    schema: "public".into(),
                    complete_table_names: false,
                },
                TuiTabSnapshot {
                    instance: "local".into(),
                    connection: "app".into(),
                    sequence: 1,
                    sql: "select 2".into(),
                    split_ratio: 45,
                    history_pane_width: 24,
                    detail_pane_width: 32,
                    database: "mydb".into(),
                    schema: "public".into(),
                    complete_table_names: false,
                },
            ],
            active_tab: Some(1),
            instance_workspace: None,
            discover_targets_ratio: 35,
            explorer_split_ratio: 20,
            explorer_pane: "instances".into(),
        };
        let mut state = sample_state();
        apply_snapshot(&mut state, &snap);
        assert_eq!(state.sql.sql_tab.tabs.len(), 2);
        assert_eq!(state.sql.sql_tab.active_tab, 1);
        assert_eq!(
            crate::common::editor::editor_text(&state.sql.sql_tab.tabs[1].editor.editor),
            "select 2"
        );
        assert_eq!(
            state.sql.sql_tab.tabs[0].session.instance.as_deref(),
            Some("local")
        );
        assert_eq!(state.focus, Pane::SQLWorkspace);

        // Sanity: focus round-trips through snapshot_from_app.
        let back = snapshot_from_app(&state);
        assert_eq!(back.focus, "sql_workspace");
        assert_eq!(back.tabs.len(), 2);
    }

    #[test]
    fn empty_tabs_restore_stays_empty() {
        let snap = TuiSessionSnapshot {
            version: TUI_SESSION_VERSION,
            focus: "explorer".into(),
            tree_width: 20,
            tree: TuiTreeSnapshot {
                expanded_instances: Vec::new(),
                cursor: None,
                active_workspace: None,
                expanded_objects: Vec::new(),
                objects_bound_instance: String::new(),
                objects_bound_connection: String::new(),
            },
            tabs: Vec::new(),
            active_tab: None,
            instance_workspace: None,
            discover_targets_ratio: 35,
            explorer_split_ratio: 20,
            explorer_pane: "instances".into(),
        };
        let mut state = sample_state();
        apply_snapshot(&mut state, &snap);
        // Restoring an empty tab snapshot leaves no tabs (no phantom default).
        assert!(state.sql.sql_tab.tabs.is_empty());
        assert_eq!(
            state.focus,
            Pane::Explorer(crate::app_shell::nav::ExplorerPane::default())
        );
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
        state.explorer.instances.set_instances(vec![
            managed_instance("local"),
            managed_instance("remote"),
        ]);
        state.explorer.instances.nodes[0].expanded = true;
        state.explorer.instances.nodes[0].connections = vec![connection("app")];
        state.explorer.instances.nodes[0].loaded = true;
        // Cursor on the "app" connection row (row 1).
        state.explorer.instances.cursor = 1;
        // Objects tree expansion keys.
        state.explorer.objects.expanded.insert("mydb".to_string());
        state.explorer.objects.expanded.insert("mydb\tpublic".to_string());
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
        assert_eq!(snap.explorer_pane, "objects");
        let iw = snap.instance_workspace.expect("iw snapshot present");
        assert_eq!(iw.section, "connections");
        assert_eq!(iw.connections_cursor, 1);
    }

    #[test]
    fn restore_applies_tree_expansion_and_focus() {
        let mut state = sample_state();
        // Tree is populated before the session is applied (startup order).
        state.explorer.instances.set_instances(vec![
            managed_instance("local"),
            managed_instance("remote"),
        ]);

        let snap = TuiSessionSnapshot {
            version: TUI_SESSION_VERSION,
            focus: "explorer".into(),
            tree_width: 20,
            tree: TuiTreeSnapshot {
                expanded_instances: vec!["remote".into()],
                cursor: Some(TuiTreeSelection::Instance {
                    instance: "remote".into(),
                }),
                active_workspace: None,
                expanded_objects: vec!["mydb".into()],
                objects_bound_instance: "local".into(),
                objects_bound_connection: "app".into(),
            },
            tabs: Vec::new(),
            active_tab: None,
            instance_workspace: Some(TuiInstanceWorkspaceSnapshot {
                section: "connections".into(),
                connections_cursor: 0,
            }),
            discover_targets_ratio: 35,
            explorer_split_ratio: 20,
            explorer_pane: "objects".into(),
        };
        // "remote" is restored expanded and its connections are not loaded yet,
        // so a LoadConnections effect is emitted to fetch its subtree.
        let effects = apply_snapshot(&mut state, &snap);
        assert_eq!(effects.len(), 1, "restored-expanded unloaded instance must emit a load effect");
        assert!(state.explorer.instances.nodes[1].expanded, "remote restored expanded");
        assert!(!state.explorer.instances.nodes[0].expanded);
        // Cursor moved onto the "remote" instance row (row 1).
        assert_eq!(state.explorer.instances.cursor, 1);
        assert_eq!(state.explorer.pane, ExplorerPane::Objects);
        assert_eq!(state.iw.pane, IwPane::Connections);
        // Objects expansion is staged for the saved bound connection, not yet
        // applied (the tree is unbound right after startup).
        assert!(state.explorer.objects.expanded.is_empty());
        assert_eq!(state.explorer.objects.restore_expanded, vec!["mydb".to_string()]);
        assert_eq!(state.explorer.objects.restore_bound_connection, "app");
    }

    #[test]
    fn restore_skips_load_when_instance_already_loaded() {
        let mut state = sample_state();
        state.explorer.instances.set_instances(vec![managed_instance("remote")]);
        // Already loaded -> restoring its expansion must not re-emit a load.
        state.explorer.instances.nodes[0].loaded = true;

        let snap = TuiSessionSnapshot {
            version: TUI_SESSION_VERSION,
            focus: "header".into(),
            tree_width: 20,
            tree: TuiTreeSnapshot {
                expanded_instances: vec!["remote".into()],
                cursor: None,
                active_workspace: None,
                expanded_objects: Vec::new(),
                objects_bound_instance: String::new(),
                objects_bound_connection: String::new(),
            },
            tabs: Vec::new(),
            active_tab: None,
            instance_workspace: None,
            discover_targets_ratio: 35,
            explorer_split_ratio: 20,
            explorer_pane: "instances".into(),
        };
        let effects = apply_snapshot(&mut state, &snap);
        assert!(effects.is_empty(), "loaded instance must not be reloaded");
    }
}
