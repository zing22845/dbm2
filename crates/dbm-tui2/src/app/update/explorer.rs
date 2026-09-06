//! Explorer messages: tree loading, instances/objects updates and the
//! object-tree binding to the active connection.

use super::UpdateResult;
use super::{box_effect, box_intent, focus_changed, sync_objects_active, sync_objects_binding};
use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app_shell::pane::Pane;
use crate::features::explorer::effect::ExplorerEffect;
use crate::features::explorer::instances::effect::InstancesEffect;
use crate::features::explorer::intent::ExplorerIntent;
use crate::features::explorer::msg::ExplorerMsg;
use crate::features::explorer::update::update as explorer_update;
use crate::features::instance_workspace::update::update as iw_update;
use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};

pub(super) fn apply(msg: AppMsg, state: &mut AppState, result: &mut UpdateResult) {
    let AppMsg::Explorer(m) = msg else {
        return;
    };
    let ExplorerMsg::Message(inner) = m;
    // The explorer feature's update is a pure by-value transition: move
    // the state out, update it, move the result back. No deep clone.
    let explorer = std::mem::take(&mut state.explorer);
    let (s, intents, effects, mut explorer_dirty) = explorer_update(inner, explorer);
    state.explorer = s;
    // Cross-feature: selecting an instance in the explorer opens the
    // instance workspace for it. This is shell-level orchestration that
    // dispatches an iw message based on the explorer's intent.
    for intent in &intents {
        if let ExplorerIntent::Instances(
            crate::features::explorer::instances::intent::InstancesIntent::OpenInstanceWorkspace {
                instance_idx,
            },
        ) = intent
        {
            let instance_name = state
                .explorer
                .instances
                .nodes
                .get(*instance_idx)
                .and_then(|n| n.instance.as_ref())
                .map(|i| i.name.clone())
                .unwrap_or_default();
            if !instance_name.is_empty() {
                // Mark this instance as the active workspace (the
                // active-row highlight + what the workspace region
                // renders), matching the original dbm's
                // `set_active_instance`. This force-expands the node, so
                // keep the expanded state consistent with the
                // connection-load state: if the instance was collapsed
                // and its connections are not loaded yet, fetch them now.
                state.explorer.instances.set_active_instance(*instance_idx);
                if state
                    .explorer
                    .instances
                    .nodes
                    .get(*instance_idx)
                    .is_some_and(|n| n.expanded && !n.loaded)
                {
                    result.effects.push(box_effect(ExplorerEffect::Instances(
                        InstancesEffect::LoadConnections {
                            instance_idx: *instance_idx,
                            instance_name: instance_name.clone(),
                        },
                    )));
                }
                let iw = std::mem::take(&mut state.iw);
                let (iw2, i, e, d) = iw_update(
                    crate::features::instance_workspace::msg::IwMessage::OpenInstance {
                        instance_name,
                    },
                    iw,
                );
                state.iw = iw2;
                explorer_dirty |= d;
                result.intents.extend(i.into_iter().map(box_intent));
                result.effects.extend(e.into_iter().map(box_effect));
                // Switch focus to the instance workspace so the user
                // sees it immediately instead of staying on explorer.
                result
                    .pending
                    .push_back(focus_changed(Pane::InstanceWorkspace(
                        crate::app_shell::nav::IwPane::Overview,
                    )));
            }
        }
        if let ExplorerIntent::Instances(
                    crate::features::explorer::instances::intent::InstancesIntent::OpenConnectionWorkspace {
                        instance_idx,
                        connection_idx,
                    },
                ) = intent
                {
                    let node = state.explorer.instances.nodes.get(*instance_idx);
                    if let Some(node) = node {
                        let instance_name = node
                            .instance
                            .as_ref()
                            .map(|i| i.name.clone())
                            .unwrap_or_default();
                        if let Some(conn) = node.connections.get(*connection_idx) {
                            // Clone what the sql message needs before mutating
                            // the tree (to release the immutable `node` borrow).
                            let connection = conn.name.clone();
                            let connection_id = conn.id.clone();
                            // The connection's configured default database is the
                            // fallback context when the connection has no tab yet.
                            let default_database = if conn.database.is_empty() {
                                Some("postgres".to_string())
                            } else {
                                Some(conn.database.clone())
                            };
                            // Mark this connection as the active workspace
                            // (the active-row highlight + what the workspace
                            // region renders), matching the original dbm's
                            // `set_active_connection`. This overwrites any
                            // previously-open instance workspace so the display
                            // switches to the SQL workspace.
                            state.explorer.instances.set_active_connection(*instance_idx, *connection_idx);
                            // Enter on a connection focuses its existing tab (or
                            // opens one if none), mirroring the original dbm's
                            // `confirm_workspace_connection(force_new=false)`.
                            let sql_msg = SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                                SqlTabMessage::FocusConnectionTab {
                                    instance: instance_name,
                                    connection,
                                    connection_id,
                                    database: None,
                                    schema: None,
                                    default_database,
                                },
                            )));
                            explorer_dirty = true;
                            result.pending.push_back(AppMsg::Sql(sql_msg));
                            // Switch focus to the workspace so the user sees the
                            // active tab immediately, mirroring the original
                            // dbm's `confirm_workspace_connection` → `focus_workspace`.
                            result.pending.push_back(focus_changed(Pane::SQLWorkspace));
                        }
                    }
                }
        if let ExplorerIntent::Instances(
            crate::features::explorer::instances::intent::InstancesIntent::NewConnectionWorkspace {
                instance_idx,
                connection_idx,
            },
        ) = intent
        {
            let node = state.explorer.instances.nodes.get(*instance_idx);
            if let Some(node) = node {
                let instance_name = node
                    .instance
                    .as_ref()
                    .map(|i| i.name.clone())
                    .unwrap_or_default();
                if let Some(conn) = node.connections.get(*connection_idx) {
                    let connection = conn.name.clone();
                    let connection_id = conn.id.clone();
                    // The connection's configured default database is the
                    // fallback context when the connection has no tab yet.
                    let default_database = if conn.database.is_empty() {
                        Some("postgres".to_string())
                    } else {
                        Some(conn.database.clone())
                    };
                    // Mark this connection as the active workspace
                    // (the active-row highlight + what the workspace
                    // region renders), matching the original dbm's
                    // `set_active_connection`.
                    state
                        .explorer
                        .instances
                        .set_active_connection(*instance_idx, *connection_idx);
                    // `n` on a connection always opens a fresh editor,
                    // mirroring the original dbm's
                    // `confirm_workspace_connection(force_new=true)`.
                    let sql_msg = SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                        SqlTabMessage::OpenConnectionTab {
                            instance: instance_name,
                            connection,
                            connection_id,
                            database: None,
                            schema: None,
                            default_database,
                        },
                    )));
                    explorer_dirty = true;
                    result.pending.push_back(AppMsg::Sql(sql_msg));
                    result.pending.push_back(focus_changed(Pane::SQLWorkspace));
                }
            }
        }
        if let ExplorerIntent::Instances(
            crate::features::explorer::instances::intent::InstancesIntent::RequestAddConnection {
                instance_idx,
            },
        ) = intent
        {
            let instance_name = state
                .explorer
                .instances
                .nodes
                .get(*instance_idx)
                .and_then(|n| n.instance.as_ref())
                .map(|i| i.name.clone())
                .unwrap_or_default();
            if !instance_name.is_empty() {
                state.explorer.instances.set_active_instance(*instance_idx);
                // Lazy-load connections if not loaded yet (needed so
                // the IW connections pane has something to display).
                if state
                    .explorer
                    .instances
                    .nodes
                    .get(*instance_idx)
                    .is_some_and(|n| !n.loaded)
                {
                    result.effects.push(box_effect(ExplorerEffect::Instances(
                        InstancesEffect::LoadConnections {
                            instance_idx: *instance_idx,
                            instance_name: instance_name.clone(),
                        },
                    )));
                }
                // Open the IW for this instance.
                let iw = std::mem::take(&mut state.iw);
                let (iw2, i, e, _d) = iw_update(
                    crate::features::instance_workspace::msg::IwMessage::OpenInstance {
                        instance_name: instance_name.clone(),
                    },
                    iw,
                );
                state.iw = iw2;
                result.intents.extend(i.into_iter().map(box_intent));
                result.effects.extend(e.into_iter().map(box_effect));
                // Switch focus to IW connections pane.
                result
                    .pending
                    .push_back(focus_changed(Pane::InstanceWorkspace(
                        crate::app_shell::nav::IwPane::Connections,
                    )));
                // Auto-trigger BeginAdd — IW connections cursor is at 0,
                // so BeginAdd opens a blank form (that's the correct
                // behavior regardless of which row the user pressed `a`
                // on in the explorer tree).
                result.pending.push_back(AppMsg::Iw(
                            crate::features::instance_workspace::msg::IwMsg::Message(
                                crate::features::instance_workspace::msg::IwMessage::Connections(
                                    crate::features::instance_workspace::connections::msg::ConnectionsMsg::Message(
                                        crate::features::instance_workspace::connections::msg::ConnectionsMessage::BeginAdd,
                                    ),
                                ),
                            ),
                        ));
            }
        }
        if let ExplorerIntent::Instances(
            crate::features::explorer::instances::intent::InstancesIntent::RequestEditConnection {
                instance_idx,
                connection_idx: _explorer_conn_idx,
            },
        ) = intent
        {
            let instance_name = state
                .explorer
                .instances
                .nodes
                .get(*instance_idx)
                .and_then(|n| n.instance.as_ref())
                .map(|i| i.name.clone())
                .unwrap_or_default();
            if !instance_name.is_empty() {
                state.explorer.instances.set_active_instance(*instance_idx);
                // Ensure the parent instance is expanded so the
                // connection row stays visible in the tree.
                let expanded = state.explorer.instances.expand();
                let _ = expanded; // no-op if already expanded
                // Lazy-load connections.
                if state
                    .explorer
                    .instances
                    .nodes
                    .get(*instance_idx)
                    .is_some_and(|n| !n.loaded)
                {
                    result.effects.push(box_effect(ExplorerEffect::Instances(
                        InstancesEffect::LoadConnections {
                            instance_idx: *instance_idx,
                            instance_name: instance_name.clone(),
                        },
                    )));
                }
                // Open IW, switch to connections pane.
                let iw = std::mem::take(&mut state.iw);
                let (iw2, i, e, _d) = iw_update(
                    crate::features::instance_workspace::msg::IwMessage::OpenInstance {
                        instance_name: instance_name.clone(),
                    },
                    iw,
                );
                state.iw = iw2;
                result.intents.extend(i.into_iter().map(box_intent));
                result.effects.extend(e.into_iter().map(box_effect));
                result
                    .pending
                    .push_back(focus_changed(Pane::InstanceWorkspace(
                        crate::app_shell::nav::IwPane::Connections,
                    )));
                // Auto-trigger BeginEdit.
                // TODO: align IW connections cursor to the specific
                // connection the user selected in the explorer so
                // BeginEdit opens the right form (currently edits the
                // connection at cursor 0).
                result.pending.push_back(AppMsg::Iw(
                            crate::features::instance_workspace::msg::IwMsg::Message(
                                crate::features::instance_workspace::msg::IwMessage::Connections(
                                    crate::features::instance_workspace::connections::msg::ConnectionsMsg::Message(
                                        crate::features::instance_workspace::connections::msg::ConnectionsMessage::BeginEdit,
                                    ),
                                ),
                            ),
                        ));
            }
        }
        // Cross-feature: opening an object (e.g. a table) from the object
        // tree. Mirroring the original dbm's double-click behavior:
        //   - a table/view/matview runs a `SELECT * FROM "schema"."table"`
        //     data query in the connection's active SQL tab (opening it
        //     if needed) and focuses the Results pane;
        //   - other objects (procedure/function/sequence) open a new SQL
        //     tab scoped to the object's database/schema.
        if let ExplorerIntent::Objects(
            crate::features::explorer::objects::intent::ObjectsIntent::OpenObject { target },
        ) = intent
        {
            let instance = state.explorer.objects.bound_instance.clone();
            let connection = state.explorer.objects.bound_connection.clone();
            if !instance.is_empty() && !connection.is_empty() {
                let connection_id = state
                    .explorer
                    .instances
                    .connection_id_by_name(&instance, &connection)
                    .unwrap_or_default();
                let sql_msg = match target.kind {
                    crate::features::explorer::objects::state::ObjectKind::Tables
                    | crate::features::explorer::objects::state::ObjectKind::Views
                    | crate::features::explorer::objects::state::ObjectKind::Matviews => {
                        SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                            SqlTabMessage::RunTableQuery {
                                instance,
                                connection,
                                connection_id,
                                database: Some(target.database.clone()),
                                schema: target.schema.clone(),
                                table: target.name.clone(),
                                table_schema: target.schema.clone(),
                            },
                        )))
                    }
                    _ => SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                        SqlTabMessage::OpenConnectionTab {
                            instance,
                            connection,
                            connection_id,
                            database: Some(target.database.clone()),
                            schema: target.schema.clone(),
                            // An explicit database is passed, so the
                            // default fallback is never used.
                            default_database: None,
                        },
                    ))),
                };
                explorer_dirty = true;
                result.pending.push_back(AppMsg::Sql(sql_msg));
                // Switch focus to the workspace so the user sees the
                // active tab / results immediately, mirroring the
                // original dbm's `confirm_workspace_connection` →
                // `focus_workspace`.
                result.pending.push_back(focus_changed(Pane::SQLWorkspace));
            }
        }
        // Cross-feature: Enter on a schema row applies it as the active
        // database/schema of the bound SQL tab (mirroring the original
        // dbm's `apply_objects_schema`). The active schema is already
        // highlighted/forced-expanded by the objects update itself.
        if let ExplorerIntent::Objects(
            crate::features::explorer::objects::intent::ObjectsIntent::ApplySchema {
                database,
                name,
            },
        ) = intent
        {
            let tab_id = state.sql.sql_tab.active_tab().map(|t| t.session.id);
            if let Some(tab_id) = tab_id {
                let sql_msg = SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::ApplyContext {
                        tab_id,
                        database: database.clone(),
                        schema: name.clone(),
                    },
                )));
                explorer_dirty = true;
                result.pending.push_back(AppMsg::Sql(sql_msg));
            }
        }
    }
    result.dirty |= explorer_dirty;
    result.intents.extend(intents.into_iter().map(box_intent));
    result.effects.extend(effects.into_iter().map(box_effect));
    // Keep the objects tree's binding + active schema in sync with the
    // active SQL tab / connection. Explorer-driven activation — e.g.
    // `ConnectionsLoaded` refining a restored active connection, or a
    // connection selected in the tree — changes `active_workspace`, so
    // the objects tree must rebind here too, not only on SQL/focus
    // changes. `Bind` is idempotent, so a redundant bind is a no-op.
    if let Some(bind) = sync_objects_binding(&mut state.explorer.objects, &state.explorer.instances)
    {
        result.pending.push_back(bind);
    }
    if let Some(effect) = sync_objects_active(&mut state.explorer.objects, &state.sql) {
        result
            .effects
            .push(box_effect(ExplorerEffect::Objects(effect)));
    }
}

#[cfg(test)]
mod tests {
    use super::super::update_unchecked;
    use super::*;

    fn explorer_instances_msg(
        m: crate::features::explorer::instances::msg::InstancesMessage,
    ) -> AppMsg {
        AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
            crate::features::explorer::msg::ExplorerMessage::Instances(
                crate::features::explorer::instances::msg::InstancesMsg::Message(m),
            ),
        ))
    }
    fn sample_managed_instance(name: &str) -> dbm_store::ManagedInstance {
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
    fn sample_connection(name: &str) -> dbm_store::InstanceConnection {
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
    fn connections_loaded_activation_binds_objects_tree() {
        use crate::features::explorer::instances::msg::InstancesMessage;
        use crate::features::explorer::instances::state::ActiveWorkspaceKind;

        let mut state = AppState::default();
        state
            .explorer
            .instances
            .set_instances(vec![sample_managed_instance("inst")]);
        state.explorer.instances.nodes[0].expanded = true;
        // A saved connection-active pending restore (set by `apply_snapshot`);
        // loading this instance's connections refines the active workspace onto
        // the connection. This is the exact session-restore path.
        state.explorer.instances.restore_active_connection =
            Some(("inst".to_string(), "c1".to_string()));

        // An explorer-driven activation: loading the instance's connections
        // refines the active workspace onto the connection (this is the path
        // the session restore / ConnectionsLoaded action takes). It must not
        // only highlight the row but also bind the objects tree.
        let r = update_unchecked(
            explorer_instances_msg(InstancesMessage::ConnectionsLoaded {
                instance_idx: 0,
                connections: vec![sample_connection("c1")],
            }),
            &mut state,
        );
        assert_eq!(
            state.explorer.instances.active_workspace,
            Some(ActiveWorkspaceKind::Connection {
                instance_idx: 0,
                conn_idx: 0
            })
        );
        // The pending queue must carry a Bind for the active connection.
        let has_bind = r.pending.iter().any(|m| {
            matches!(
                m,
                AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
                    crate::features::explorer::msg::ExplorerMessage::Objects(
                        crate::features::explorer::objects::msg::ObjectsMsg::Message(
                            crate::features::explorer::objects::msg::ObjectsMessage::Bind {
                                instance, connection
                            }
                        )
                    )
                )) if instance == "inst" && connection == "c1"
            )
        });
        assert!(
            has_bind,
            "explorer-driven activation must request an objects bind"
        );
    }
    #[test]
    fn opening_a_collapsed_unloaded_instance_loads_its_connections() {
        use crate::features::explorer::instances::msg::InstancesMessage;
        use crate::features::explorer::instances::state::ActiveWorkspaceKind;

        let mut state = AppState::default();
        state
            .explorer
            .instances
            .set_instances(vec![sample_managed_instance("inst")]);
        // Collapsed and not loaded (right after startup).
        state.explorer.instances.nodes[0].expanded = false;
        state.explorer.instances.nodes[0].loaded = false;
        state.focus = Pane::Explorer(crate::app_shell::nav::ExplorerPane::default());

        // Selecting the collapsed instance force-expands it (active workspace)
        // and must keep the expanded/loaded state consistent by requesting its
        // connections (a LoadConnections effect).
        let r = update_unchecked(explorer_instances_msg(InstancesMessage::Select), &mut state);
        assert_eq!(
            state.explorer.instances.active_workspace,
            Some(ActiveWorkspaceKind::Instance(0))
        );
        assert!(state.explorer.instances.nodes[0].expanded);
        assert!(
            !r.effects.is_empty(),
            "force-expanding an unloaded instance must request its connections"
        );
    }
}
