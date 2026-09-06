//! Session restore: apply a persisted snapshot back onto the [`AppState`].
use super::snapshot::{
    approx_discover_body_height, approx_explorer_body_height, approx_sql_body_height,
};
use dbm_store::{TuiSessionSnapshot, TuiTreeSelection};

use crate::app::action::Action;
use crate::app::state::AppState;
use crate::app_shell::effect::ErasedEffect;
use crate::app_shell::nav::{ExplorerPane, IwPane};
use crate::app_shell::pane::{Pane, pane_from_name};

pub(super) fn apply_snapshot(
    state: &mut AppState,
    snapshot: &TuiSessionSnapshot,
) -> Vec<Box<dyn ErasedEffect<Action>>> {
    use crate::features::sql_workspace::sql_tab::editor::state::EditorState;

    // Rebuild the tab list from the snapshot. `t.sequence` is a per-connection
    // ordinal (each connection's tabs restart at 1), so it must NOT be reused
    // as the global `session.id` — that would collide across connections (two
    // connections' first tab both become `id == 1`), and `index_of` (which
    // returns the first match) would then route editor/context messages to the
    // wrong tab, leaving some connections unable to open their context picker.
    // Assign a fresh unique global id per restored tab instead.
    let mut restored: Vec<crate::features::sql_workspace::sql_tab::state::SqlTab> =
        Vec::with_capacity(snapshot.tabs.len());
    for t in &snapshot.tabs {
        let id = state.sql.sql_tab.next_session_id();
        restored.push(crate::features::sql_workspace::sql_tab::state::SqlTab {
            session: crate::features::sql_workspace::sql_tab::session::TabSession {
                id,
                sequence: t.sequence as usize,
                instance: non_empty(t.instance.clone()),
                connection: non_empty(t.connection.clone()),
                connection_id: None,
                database: non_empty(t.database.clone()),
                schema: non_empty(t.schema.clone()),
            },
            focus: crate::features::sql_workspace::sql_tab::state::SqlFocus::default(),
            upper_pane: crate::features::sql_workspace::sql_tab::state::SqlFocus::Editor,
            splitter: {
                let mut s = crate::features::sql_workspace::sql_tab::splitter::state::SqlTabSplitterState::default();
                // The stored split is a percentage; materialize it to rows
                // against the current (approximate) body height.
                s.set_editor_top_pct(t.split_ratio, approx_sql_body_height(state));
                s.set_history_pane_width(t.history_pane_width);
                s
            },
            complete_table_names: t.complete_table_names,
            clear_editor_after_run: false,
            editor: {
                // Keep the editor's TblCmp flag in sync with the tab's flag on
                // restore, otherwise the completion engine would still offer
                // keywords at a table-intent slot even though the header shows
                // `TblCmp: ON`.
                let mut ed = EditorState::with_sql(&t.sql);
                ed.complete_table_names = t.complete_table_names;
                ed
            },
            results: crate::features::sql_workspace::sql_tab::results::state::ResultsState::new(),
            history: {
                let mut h =
                    crate::features::sql_workspace::sql_tab::history::state::HistoryState::default();
                // Mirror the original dbm: the detail pane width is persisted and
                // restored (clamped to its allowed range).
                h.splitter.set_detail_pane_width(t.detail_pane_width);
                h
            },
        });
    }
    let tabs = restored;

    // Replace the default single tab with the restored set. When there are no
    // persisted tabs, keep the default empty tab.
    if !tabs.is_empty() {
        // Keep the restored active tab only if it is in range; otherwise fall
        // back to the first restored tab (there is at least one).
        let active = snapshot.active_tab.filter(|&i| i < tabs.len()).unwrap_or(0);
        state.sql.sql_tab.tabs = tabs;
        // `next_tab_id` was already bumped past every restored tab while they
        // were allocated above (`next_session_id`), so a newly opened tab can
        // never reuse a restored `session.id`.
        state.sql.sql_tab.active_tab = Some(active);
        // Restore the active connection so the tab strip shows the restored
        // tabs. `active_connection` gates `visible_tab_indices()`; without it
        // the strip renders nothing and the previously-open tabs appear lost.
        // Pick the active tab's connection, falling back to the first tab.
        // Set the field directly (not via `activate_connection`) so the
        // snapshot's `active_tab` is preserved exactly.
        let active_session = state
            .sql
            .sql_tab
            .tabs
            .get(active)
            .or_else(|| state.sql.sql_tab.tabs.first());
        if let Some(s) = active_session
            && let (Some(instance), Some(connection)) = (&s.session.instance, &s.session.connection)
        {
            state.sql.sql_tab.active_connection = Some((instance.clone(), connection.clone()));
        }
    }

    // Resolve the saved parent pane, then fold the persisted sub-pane
    // (explorer instances/objects, iw overview/connections) back in and route
    // everything through the single focus choke point. `pane_from_name`
    // discards the sub-pane, so the sub-pane is re-attached here from the
    // snapshot before `set_focus` — that keeps `state.focus` and the feature
    // sub-panes in lockstep instead of letting them drift apart (the restore
    // bug that mis-routed `j`/`k`/Ctrl+h when closing on the connections or
    // objects pane).
    let mut focus = pane_from_name(&snapshot.focus).unwrap_or(Pane::Header);
    match snapshot.explorer_pane.as_str() {
        "objects" if matches!(focus, Pane::Explorer(_)) => {
            focus = Pane::Explorer(ExplorerPane::Objects);
        }
        _ => {}
    }
    if let Some(iw) = &snapshot.instance_workspace
        && matches!(focus, Pane::InstanceWorkspace(_))
    {
        let sub = match iw.section.as_str() {
            "connections" => IwPane::Connections,
            _ => IwPane::Overview,
        };
        focus = Pane::InstanceWorkspace(sub);
    }
    state.set_focus(focus);

    // Restore the Explorer / workspace splitter width (persisted in `tree_width`),
    // clamped to its allowed range so it survives any terminal-width change.
    state.splitter.set_explorer_pane_width(snapshot.tree_width);

    // Restore the discover targets/results splitter (persisted as a percentage),
    // materialized to rows against the current (approximate) body height.
    state.discover.splitter.set_targets_height_pct(
        snapshot.discover_targets_ratio,
        approx_discover_body_height(state),
    );

    // Restore the explorer instances/objects splitter (persisted as a
    // percentage), materialized to rows against the current body height.
    state.explorer.splitter.set_instances_height_pct(
        snapshot.explorer_split_ratio,
        approx_explorer_body_height(state),
    );

    // Restore instance expansion + cursor by name. Expansion is applied to the
    // freshly-loaded tree nodes; the cursor resolves to the owning instance row
    // (connections load lazily, so a connection cursor focuses its instance).
    let mut restore_effects: Vec<Box<dyn ErasedEffect<Action>>> = Vec::new();
    apply_instances_tree(state, snapshot, &mut restore_effects);
    apply_objects_expansion(state, snapshot);

    // Restore the active workspace (the active-row highlight for an instance or
    // connection). Connections load lazily, so a saved connection-active
    // degrades to its instance workspace here (the instance node is the
    // authoritative marker); it is refined once the connection rows load.
    // Opening the instance workspace context (`iw.instance_name`) prevents the
    // overview from showing the "select an instance" empty hint after a restart.
    if let Some(aw) = &snapshot.tree.active_workspace {
        let instance_name = match aw {
            TuiTreeSelection::Instance { instance } => instance,
            TuiTreeSelection::Connection { instance, .. } => instance,
        };
        if let Some(idx) = state.explorer.instances.nodes.iter().position(|n| {
            n.instance
                .as_ref()
                .is_some_and(|i| &i.name == instance_name)
        }) {
            // For a saved connection-active, don't eagerly downgrade to the
            // instance. Instead remember the connection and wait for the
            // connections to load; `ConnectionsLoaded` then activates the
            // connection if found, else falls back to its parent instance.
            // An instance-active is restored directly.
            match aw {
                TuiTreeSelection::Connection { connection, .. } => {
                    state.explorer.instances.restore_active_connection =
                        Some((instance_name.clone(), connection.clone()));
                }
                TuiTreeSelection::Instance { .. } => {
                    state.explorer.instances.set_active_instance(idx);
                }
            }
            // Restore the instance-workspace context so the overview renders
            // the instance instead of an empty prompt. The overview keys off
            // `iw.instance` (the ManagedInstance), not just the name, so mirror
            // the node's instance data onto it.
            state.iw.instance_name = instance_name.clone();
            // The connections sub-state tracks its own `instance_name` copy
            // (used by TestForm / Save etc.). OpenInstance normally sets it
            // via ConnectionsMessage::Load, but session restore bypasses that
            // path — sync it here so form actions don't dispatch with an empty
            // instance name.
            state.iw.connections.instance_name = instance_name.clone();
            if let Some(node) = state.explorer.instances.nodes.get(idx)
                && let Some(inst) = &node.instance
            {
                state.iw.overview.instance = Some(inst.clone());
            }
            // Load the connections list so the instance workspace's connections
            // panel shows them (the overview connection count comes from here),
            // not just the expanded explorer rows.
            restore_effects.push(Box::new(
                crate::features::instance_workspace::connections::effect::ConnectionsEffect::LoadConnections {
                    instance_name: instance_name.clone(),
                },
            ) as Box<dyn ErasedEffect<Action>>);
            // Eagerly load the explorer tree's connections for the active
            // instance too. This fires `ConnectionsLoaded`, which refines the
            // restored active workspace from the instance onto the saved
            // connection (so the active highlight ends up on the connection,
            // not its parent)
            // without waiting for the user to expand the node manually.
            restore_effects.push(Box::new(
                crate::features::explorer::instances::effect::InstancesEffect::LoadConnections {
                    instance_idx: idx,
                    instance_name: instance_name.clone(),
                },
            ) as Box<dyn ErasedEffect<Action>>);
        }
    }

    // Restore the connections cursor. The list is still empty here (it loads
    // lazily), so don't clamp the saved cursor against it — remember it and
    // apply it once `Loaded` populates the list. The sub-pane itself was
    // already restored through `set_focus` above, which keeps `state.focus`
    // and `state.iw.pane` in lockstep.
    if let Some(iw) = &snapshot.instance_workspace {
        state.iw.connections.restore_cursor = Some(iw.connections_cursor);
    }

    restore_effects
}
/// Restore the instances tree's expansion set and cursor from the snapshot.
/// Returns `LoadConnections` effects for every restored-expanded instance whose
/// connections are not yet loaded, so the lazily-loaded subtrees are fetched
/// right after startup (matching the interactive `Expand` behavior).
pub(super) fn apply_instances_tree(
    state: &mut AppState,
    snapshot: &TuiSessionSnapshot,
    effects: &mut Vec<Box<dyn ErasedEffect<Action>>>,
) {
    let expanded: std::collections::HashSet<String> =
        snapshot.tree.expanded_instances.iter().cloned().collect();
    for (idx, node) in state.explorer.instances.nodes.iter_mut().enumerate() {
        let name = node
            .instance
            .as_ref()
            .map(|i| i.name.clone())
            .unwrap_or_default();
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
        if let Some(inst) = &node.instance
            && &inst.name == instance_name
        {
            state.explorer.instances.cursor = row;
            // Note: the instances tree does not vertically scroll (it renders
            // all nodes from the top), so `scroll` stays 0. Setting it to the
            // restored row here would make `row_at` offset every mouse click.
            return;
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
pub(super) fn apply_objects_expansion(state: &mut AppState, snapshot: &TuiSessionSnapshot) {
    // Restore the active database immediately (so its row is forced expanded)
    // but defer the active schema: the tree is unbound at startup and its
    // schemas load lazily, so the schema is validated (and degraded if it no
    // longer exists) once `SchemasLoaded` arrives — the same wait-then-activate
    // pattern used for connections. `databases_loaded` handles fetching schemas
    // for the active database after the tree binds.
    state.explorer.objects.defer_active(
        snapshot.tree.objects_active_db.clone(),
        snapshot.tree.objects_active_schema.clone(),
    );
    if snapshot.tree.expanded_objects.is_empty() {
        return;
    }
    // Only restore the ACTIVE database's expansion. Other databases that were
    // expanded in the previous session are left collapsed on reopen: their
    // schemas are not fetched eagerly, so re-expanding them would show only the
    // Extensions group (matching the original dbm, which does not persist the
    // objects tree's arbitrary expansion). The active database is already forced
    // expanded via the active path; the active schema's keys are re-applied here
    // so the active schema's groups stay open.
    let active_db = snapshot
        .tree
        .objects_active_db
        .as_deref()
        .unwrap_or_default();
    let keep: Vec<String> = snapshot
        .tree
        .expanded_objects
        .iter()
        .filter(|k| k.split('\t').next().is_some_and(|db| db == active_db))
        .cloned()
        .collect();
    state.explorer.objects.restore_expanded = keep;
    state.explorer.objects.restore_bound_connection =
        snapshot.tree.objects_bound_connection.clone();
}
pub(super) fn non_empty(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(test)]
mod tests {
    use super::super::snapshot::snapshot_from_app;
    use super::*;
    use dbm_store::{
        TUI_SESSION_VERSION, TuiInstanceWorkspaceSnapshot, TuiTabSnapshot, TuiTreeSnapshot,
    };

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
                objects_active_db: None,
                objects_active_schema: None,
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
        assert_eq!(state.sql.sql_tab.active_tab, Some(1));
        assert_eq!(
            crate::common::editor::editor_text(&state.sql.sql_tab.tabs[1].editor.editor),
            "select 2"
        );
        assert_eq!(
            state.sql.sql_tab.tabs[0].session.instance.as_deref(),
            Some("local")
        );
        // The active connection must be restored so the tab strip shows the
        // previously-open tabs (it gates `visible_tab_indices`). Without this
        // the restored tabs render as empty and appear lost after a restart.
        assert_eq!(
            state.sql.sql_tab.active_connection.as_ref(),
            Some(&("local".to_string(), "app".to_string()))
        );
        assert_eq!(state.sql.sql_tab.visible_tab_count(), 2);
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
                objects_active_db: None,
                objects_active_schema: None,
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

    #[test]
    fn restore_applies_tree_expansion_and_focus() {
        let mut state = sample_state();
        // Tree is populated before the session is applied (startup order).
        state
            .explorer
            .instances
            .set_instances(vec![managed_instance("local"), managed_instance("remote")]);

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
                objects_active_db: Some("mydb".into()),
                objects_active_schema: Some("public".into()),
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
        assert_eq!(
            effects.len(),
            1,
            "restored-expanded unloaded instance must emit a load effect"
        );
        assert!(
            state.explorer.instances.nodes[1].expanded,
            "remote restored expanded"
        );
        assert!(!state.explorer.instances.nodes[0].expanded);
        // Cursor moved onto the "remote" instance row (row 1).
        assert_eq!(state.explorer.instances.cursor, 1);
        // The saved explorer sub-pane (objects) is folded into the focus and
        // mirrored onto the feature sub-pane so input routing matches rendering.
        assert_eq!(
            state.focus,
            Pane::Explorer(ExplorerPane::Objects),
            "focus carries the restored explorer sub-pane"
        );
        assert_eq!(state.explorer.pane, ExplorerPane::Objects);
        // The saved iw section only applies when the saved focus was the
        // instance workspace; here focus was the explorer, so the iw sub-pane
        // stays at its default (it is not the focused pane).
        assert_eq!(state.iw.pane, IwPane::Overview);
        // Objects expansion is staged for the saved bound connection, not yet
        // applied (the tree is unbound right after startup).
        assert!(state.explorer.objects.expanded.is_empty());
        assert_eq!(
            state.explorer.objects.restore_expanded,
            vec!["mydb".to_string()]
        );
        assert_eq!(state.explorer.objects.restore_bound_connection, "app");
    }

    #[test]
    fn restore_collapses_databases_that_are_not_the_active_database() {
        // The reported bug: database A was expanded but the active schema is
        // under database B. On reopen, A must NOT be re-expanded (its schemas
        // are not fetched eagerly, so it would only show the Extensions group);
        // only the active database B's expansion is restored.
        let mut state = sample_state();
        state
            .explorer
            .instances
            .set_instances(vec![managed_instance("local")]);
        let snap = TuiSessionSnapshot {
            version: TUI_SESSION_VERSION,
            focus: "explorer".into(),
            tree_width: 20,
            tree: TuiTreeSnapshot {
                expanded_instances: Vec::new(),
                cursor: None,
                active_workspace: None,
                // A is expanded; B is the active database (active schema under B).
                expanded_objects: vec![
                    "a".into(),
                    "b".into(),
                    "b\tpublic".into(),
                    "b\tpublic\tTables".into(),
                ],
                objects_bound_instance: "local".into(),
                objects_bound_connection: "app".into(),
                objects_active_db: Some("b".into()),
                objects_active_schema: Some("public".into()),
            },
            tabs: Vec::new(),
            active_tab: None,
            instance_workspace: None,
            discover_targets_ratio: 35,
            explorer_split_ratio: 20,
            explorer_pane: "objects".into(),
        };
        apply_snapshot(&mut state, &snap);
        // Only B's expansion is staged; A is dropped so it collapses on rebind.
        assert_eq!(
            state.explorer.objects.restore_expanded,
            vec![
                "b".to_string(),
                "b\tpublic".to_string(),
                "b\tpublic\tTables".to_string()
            ],
            "non-active database A is not re-expanded"
        );
        // The active database is forced expanded via the active path.
        assert_eq!(state.explorer.objects.active_db.as_deref(), Some("b"));
    }

    #[test]
    fn restore_applies_active_workspace() {
        use crate::features::explorer::instances::state::ActiveWorkspaceKind;
        let mut state = sample_state();
        state
            .explorer
            .instances
            .set_instances(vec![managed_instance("local"), managed_instance("remote")]);

        let snap = TuiSessionSnapshot {
            version: TUI_SESSION_VERSION,
            focus: "explorer".into(),
            tree_width: 20,
            tree: TuiTreeSnapshot {
                expanded_instances: Vec::new(),
                cursor: None,
                active_workspace: Some(TuiTreeSelection::Instance {
                    instance: "remote".into(),
                }),
                expanded_objects: Vec::new(),
                objects_bound_instance: String::new(),
                objects_bound_connection: String::new(),
                objects_active_db: None,
                objects_active_schema: None,
            },
            tabs: Vec::new(),
            active_tab: None,
            instance_workspace: None,
            discover_targets_ratio: 35,
            explorer_split_ratio: 20,
            explorer_pane: "instances".into(),
        };
        apply_snapshot(&mut state, &snap);
        assert_eq!(
            state.explorer.instances.active_workspace,
            Some(ActiveWorkspaceKind::Instance(1)),
            "active workspace must be restored onto the matching instance by name"
        );
    }

    #[test]
    fn restore_skips_load_when_instance_already_loaded() {
        let mut state = sample_state();
        state
            .explorer
            .instances
            .set_instances(vec![managed_instance("remote")]);
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
                objects_active_db: None,
                objects_active_schema: None,
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

    #[test]
    fn restore_keeps_iw_focus_subpane_and_queues_connections_cursor() {
        let mut state = sample_state();
        state
            .explorer
            .instances
            .set_instances(vec![managed_instance("local")]);
        state.explorer.instances.set_active_instance(0);

        let snap = TuiSessionSnapshot {
            version: TUI_SESSION_VERSION,
            focus: "instance_workspace".into(),
            tree_width: 20,
            tree: TuiTreeSnapshot {
                expanded_instances: Vec::new(),
                cursor: None,
                active_workspace: None,
                expanded_objects: Vec::new(),
                objects_bound_instance: String::new(),
                objects_bound_connection: String::new(),
                objects_active_db: None,
                objects_active_schema: None,
            },
            tabs: Vec::new(),
            active_tab: None,
            instance_workspace: Some(TuiInstanceWorkspaceSnapshot {
                section: "connections".into(),
                connections_cursor: 1,
            }),
            discover_targets_ratio: 35,
            explorer_split_ratio: 20,
            explorer_pane: "instances".into(),
        };
        apply_snapshot(&mut state, &snap);
        // The view renders from `state.iw.pane`; keyboard input is routed from
        // `state.focus`'s sub-pane. Both must be Connections or `j`/`k`/Ctrl+h
        // would be routed to the overview handler.
        assert_eq!(state.iw.pane, IwPane::Connections);
        assert_eq!(
            state.focus,
            Pane::InstanceWorkspace(IwPane::Connections),
            "focus sub-pane must mirror the restored iw sub-pane"
        );
        // The connections list is empty at restore time (it loads lazily), so
        // the saved cursor is queued and applied once connections arrive.
        assert_eq!(state.iw.connections.restore_cursor, Some(1));
        assert_eq!(state.iw.connections.cursor, 0);
    }

    #[test]
    fn restore_keeps_explorer_subpane_in_lockstep_with_focus() {
        let mut state = sample_state();
        state
            .explorer
            .instances
            .set_instances(vec![managed_instance("local")]);
        state.explorer.instances.set_active_instance(0);

        // Close on the objects sub-pane. This is the exact same latent desync
        // the connections pane had: restore used to set focus to the default
        // instances sub-pane while separately restoring `explorer.pane` to
        // objects, mis-routing j/k/Ctrl+j to instances.
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
                objects_active_db: None,
                objects_active_schema: None,
            },
            tabs: Vec::new(),
            active_tab: None,
            instance_workspace: None,
            discover_targets_ratio: 35,
            explorer_split_ratio: 20,
            explorer_pane: "objects".into(),
        };
        apply_snapshot(&mut state, &snap);
        assert_eq!(
            state.focus,
            Pane::Explorer(ExplorerPane::Objects),
            "restored explorer focus carries the saved objects sub-pane"
        );
        assert_eq!(
            state.explorer.pane,
            ExplorerPane::Objects,
            "feature sub-pane mirrors the restored focus"
        );
    }

    #[test]
    fn restore_keeps_persisted_splitter_widths() {
        // Mirrors the original dbm's restore_sql_tab_keeps_persisted_pane_widths:
        // the A (history) and B (detail) splitter widths survive a save/load
        // round trip, clamped to their allowed ranges.
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
                objects_active_db: None,
                objects_active_schema: None,
            },
            tabs: vec![TuiTabSnapshot {
                instance: "local".into(),
                connection: "app".into(),
                sequence: 0,
                sql: "select 1".into(),
                split_ratio: 50,
                history_pane_width: 60,
                detail_pane_width: 66,
                database: "mydb".into(),
                schema: "public".into(),
                complete_table_names: false,
            }],
            active_tab: Some(0),
            instance_workspace: None,
            discover_targets_ratio: 35,
            explorer_split_ratio: 20,
            explorer_pane: "instances".into(),
        };
        let mut state = sample_state();
        state.term_height = 40; // so 55% of the body maps to a concrete row count
        apply_snapshot(&mut state, &snap);
        assert_eq!(state.sql.sql_tab.tabs.len(), 1);
        let tab = &state.sql.sql_tab.tabs[0];
        // The persisted percentage is materialized to absolute rows on restore.
        let track_h = approx_sql_body_height(&state); // 40 - 6 = 34
        assert_eq!(tab.splitter.editor_top_height, (34 * 50) / 100);
        assert_eq!(tab.splitter.editor_top_pct(track_h), 50);
        assert_eq!(tab.splitter.history_pane_width, 60);
        assert_eq!(tab.history.splitter.detail_pane_width, 66);

        // Out-of-range values are clamped on restore.
        let snap2 = {
            let mut s = snap;
            s.tabs[0].history_pane_width = 9999;
            s.tabs[0].detail_pane_width = 9999;
            s
        };
        apply_snapshot(&mut state, &snap2);
        assert!(state.sql.sql_tab.tabs[0].splitter.history_pane_width <= 200);
        assert!(state.sql.sql_tab.tabs[0].history.splitter.detail_pane_width <= 72);
    }
}
