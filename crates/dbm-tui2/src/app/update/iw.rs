//! Instance-workspace messages: overview and connections updates.

use super::UpdateResult;
use super::{box_effect, box_intent, explorer_load_instances_msg};
use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app_shell::pane::Pane;
use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
use crate::features::instance_workspace::update::update as iw_update;

pub(super) fn apply(msg: AppMsg, state: &mut AppState, result: &mut UpdateResult) {
    let AppMsg::Iw(m) = msg else {
        return;
    };
    // A confirm-unregister / confirm-delete arrived, so the confirm
    // modal should close. The shell owns the modal, so this is shell
    // orchestration here.
    if matches!(
                &m,
                IwMsg::Message(IwMessage::UnregisterInstance { .. })
                    | IwMsg::Message(IwMessage::Connections(
                        crate::features::instance_workspace::connections::msg::ConnectionsMsg::Message(
                            crate::features::instance_workspace::connections::msg::ConnectionsMessage::DeleteConnection { .. }
                        )
                    ))
            ) {
                state.modal = None;
                result.dirty = true;
            }
    // When an unregister completes, drop out of the (now removed)
    // instance workspace, refresh the explorer tree, and return focus
    // to the explorer (matching the original dbm's post-unregister
    // `reload_tree` + focus return).
    if let IwMsg::Message(IwMessage::Unregistered { instance }) = &m {
        tracing::debug!(
            instance,
            "shell: instance unregistered; returning to explorer"
        );
        result.pending.push_back(explorer_load_instances_msg());
        result.pending.push_back(AppMsg::Shell(
            crate::app_shell::msg::ShellMsg::FocusChanged {
                pane: Pane::Explorer(crate::app_shell::nav::ExplorerPane::default()),
            },
        ));
    }
    let IwMsg::Message(inner) = m;
    // The instance workspace feature's update is a pure by-value
    // transition: move the state out, update it, move the result back.
    // No deep clone.
    let iw = std::mem::take(&mut state.iw);
    let (s, intents, effects, d) = iw_update(inner, iw);
    state.iw = s;
    result.dirty |= d;
    // A connection was added/edited/deleted inside the instance
    // workspace: refresh the explorer tree for that instance so the
    // change shows up on the left immediately (matching the original
    // dbm's `load_instance_connections` on save). This is shell-level
    // orchestration between the iw and explorer features.
    for intent in &intents {
        if let crate::features::instance_workspace::intent::IwIntent::Connections(
                    crate::features::instance_workspace::connections::intent::ConnectionsIntent::ConnectionsChanged {
                        instance_name,
                    },
                ) = intent
                {
                    let instance_idx = state
                        .explorer
                        .instances
                        .nodes
                        .iter()
                        .position(|n| {
                            n.instance
                                .as_ref()
                                .is_some_and(|i| i.name == *instance_name)
                        });
                    if let Some(instance_idx) = instance_idx {
                        result.pending.push_back(AppMsg::Explorer(
                            crate::features::explorer::msg::ExplorerMsg::Message(
                                crate::features::explorer::msg::ExplorerMessage::Instances(
                                    crate::features::explorer::instances::msg::InstancesMsg::Message(
                                        crate::features::explorer::instances::msg::InstancesMessage::RefreshConnections {
                                            instance_idx,
                                        },
                                    ),
                                ),
                            ),
                        ));
                    }
                }
    }
    result.intents.extend(intents.into_iter().map(box_intent));
    result.effects.extend(effects.into_iter().map(box_effect));
}

#[cfg(test)]
mod tests {
    use super::super::sync_objects_binding;
    use super::super::{update, update_unchecked};
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
    fn sync_objects_binding_follows_active_connection() {
        use crate::features::explorer::instances::state::InstancesState;
        let mut objects = crate::features::explorer::objects::state::ObjectsState::default();
        let mut instances = InstancesState::default();
        instances.set_instances(vec![sample_managed_instance("inst")]);
        instances.nodes[0].loaded = true;
        instances.nodes[0].connections = vec![sample_connection("c1")];

        // Active workspace is a connection -> bind message returned (binding
        // differs from empty).
        instances.set_active_connection(0, 0);
        let bind = sync_objects_binding(&mut objects, &instances)
            .expect("active connection should request a bind");
        assert!(matches!(
            bind,
            AppMsg::Explorer(crate::features::explorer::msg::ExplorerMsg::Message(
                crate::features::explorer::msg::ExplorerMessage::Objects(
                    crate::features::explorer::objects::msg::ObjectsMsg::Message(
                        crate::features::explorer::objects::msg::ObjectsMessage::Bind { instance, connection }
                    )
                )
            )) if instance == "inst" && connection == "c1"
        ));

        // After bind is applied, the same active connection yields no rebind.
        objects.sync_binding("inst".into(), "c1".into());
        assert!(sync_objects_binding(&mut objects, &instances).is_none());

        // Active workspace is an instance -> objects are unbound (prompt shown).
        instances.set_active_instance(0);
        assert!(sync_objects_binding(&mut objects, &instances).is_none());
        assert!(
            objects.bound_connection.is_empty(),
            "instance-active must unbind objects"
        );
    }
    #[test]
    fn delete_connection_closes_confirm_modal() {
        use crate::features::instance_workspace::connections::msg::{
            ConnectionsMessage, ConnectionsMsg,
        };
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};

        let mut state = AppState::default();
        // Focus is on the instance workspace (where the delete-confirm modal
        // was opened), so the DeleteConnection message passes the focus guard.
        state.focus = Pane::InstanceWorkspace(crate::app_shell::nav::IwPane::Connections);
        state.modal = Some(crate::app::state::ModalKind::DeleteConnectionConfirm {
            instance: "inst".into(),
            connection: "conn".into(),
        });
        let msg = AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
            ConnectionsMsg::Message(ConnectionsMessage::DeleteConnection {
                instance_name: "inst".into(),
                connection_name: "conn".into(),
            }),
        )));
        update(msg, &mut state);
        assert!(state.modal.is_none(), "confirm modal must close on delete");
    }
    #[test]
    fn new_connection_tab_from_explorer_sequences_continuously() {
        use crate::features::explorer::instances::msg::InstancesMessage;

        let mut state = AppState::default();
        state
            .explorer
            .instances
            .set_instances(vec![sample_managed_instance("inst")]);
        // Expand the instance and load one connection so `cursor_selection`
        // resolves to a connection row.
        state.explorer.instances.nodes[0].expanded = true;
        state.explorer.instances.nodes[0].loaded = true;
        state.explorer.instances.nodes[0].connections = vec![sample_connection("c1")];
        state.explorer.instances.cursor = 1; // on the connection row
        state.focus = Pane::Explorer(crate::app_shell::nav::ExplorerPane::default());

        // Apply a message and drain the resulting `pending` queue exactly like
        // the event loop does, so intent-dispatched messages (e.g. opening a
        // tab) take effect within the same logical round.
        fn drain(state: &mut AppState, msg: AppMsg) {
            let mut queue = std::collections::VecDeque::from([msg]);
            while let Some(m) = queue.pop_front() {
                let r = update_unchecked(m, state);
                queue.extend(r.pending);
            }
        }

        // Enter on the connection opens the first tab (<sql 1>).
        drain(&mut state, explorer_instances_msg(InstancesMessage::Select));
        assert_eq!(state.sql.sql_tab.tabs.len(), 1);
        assert_eq!(state.sql.sql_tab.tabs[0].session.sequence, 1);

        // `n` always opens a fresh tab -> <sql 2>, then <sql 3>.
        drain(
            &mut state,
            explorer_instances_msg(InstancesMessage::NewConnectionTab),
        );
        assert_eq!(state.sql.sql_tab.tabs.len(), 2);
        assert_eq!(state.sql.sql_tab.tabs[1].session.sequence, 2);

        drain(
            &mut state,
            explorer_instances_msg(InstancesMessage::NewConnectionTab),
        );
        assert_eq!(state.sql.sql_tab.tabs.len(), 3);
        assert_eq!(state.sql.sql_tab.tabs[2].session.sequence, 3);
    }
}
