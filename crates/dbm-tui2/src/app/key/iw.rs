//! Instance-workspace key bindings: Tab pane cycling and forwarding into
//! the feature's `input`, plus the confirm-unregister/delete helpers.
//!
//! Part of the split keyboard layer; the entry points are
//! re-exported from the parent [`super`] module.

use crate::app::msg::AppMsg;
use crate::app::state::ModalKind;
use crate::app_shell::msg::ShellMsg;
use crate::app_shell::nav::IwPane;
use crate::app_shell::pane::Pane;
use crate::features::instance_workspace::connections::msg::{ConnectionsMessage, ConnectionsMsg};
use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
use crate::features::instance_workspace::state::IwState;
use crossterm::event::{KeyCode, KeyEvent};
/// Instance workspace key bindings, routed by the active sub-pane `sub`
/// (overview / connections), or the connection form when one is open.
///
/// All per-pane key mapping and feature-state reads live in the feature's
/// `input::key_to_msg`; this shell keeps only the `Tab` pane navigation and the
/// pure conversion of the returned action into an `AppMsg` (opening a confirm
/// modal when the feature requests one).
pub(crate) fn iw_key(key: KeyEvent, sub: IwPane, state: &IwState) -> Option<AppMsg> {
    // `Tab` cycles the instance-workspace sub-panes (overview <-> connections),
    // mirroring the explorer's Tab behavior. This is shell-level focus, so it
    // is handled here rather than in the feature.
    if key.code == KeyCode::Tab {
        return Some(AppMsg::Shell(ShellMsg::FocusChanged {
            pane: Pane::InstanceWorkspace(sub.next()),
        }));
    }
    match crate::features::instance_workspace::input::key_to_msg(key, sub, state) {
        Some(crate::features::instance_workspace::input::IwInput::Message(msg)) => Some(iw(msg)),
        Some(crate::features::instance_workspace::input::IwInput::OpenUnregisterConfirm {
            instance,
        }) => Some(AppMsg::OpenModal(ModalKind::UnregisterInstanceConfirm {
            instance,
        })),
        Some(crate::features::instance_workspace::input::IwInput::OpenDeleteConfirm {
            instance,
            connection,
        }) => Some(AppMsg::OpenModal(ModalKind::DeleteConnectionConfirm {
            instance,
            connection,
        })),
        None => None,
    }
}

fn iw(msg: IwMessage) -> AppMsg {
    AppMsg::Iw(IwMsg::Message(msg))
}

/// Confirm-unregister helper: the shell closes the modal and dispatches the
/// unregister to the instance workspace. The shell closes the modal when it
/// sees the `UnregisterInstance` message.
pub(crate) fn close_and_unregister(instance: String) -> AppMsg {
    iw(IwMessage::UnregisterInstance { instance })
}

/// Confirm-delete-connection helper: dispatch the delete to the connections
/// panel. The shell closes the modal when it sees the `DeleteConnection`
/// message (matching the unregister confirm flow).
pub(crate) fn confirm_delete_connection(instance: String, connection: String) -> AppMsg {
    iw(IwMessage::Connections(ConnectionsMsg::Message(
        ConnectionsMessage::DeleteConnection {
            instance_name: instance,
            connection_name: connection,
        },
    )))
}

#[cfg(test)]
mod tests {

    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::app::key::key_to_msg;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }
    #[test]
    fn d_in_connections_opens_delete_confirm_modal() {
        use crate::app::state::ModalKind;
        use crate::features::instance_workspace::connections::state::ConnectionsState;
        use dbm_store::InstanceConnection;

        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Connections);
        state.iw.instance_name = "inst".into();
        state.iw.connections = ConnectionsState {
            instance_name: "inst".into(),
            connections: vec![InstanceConnection {
                id: "1".into(),
                instance_id: "1".into(),
                name: "conn".into(),
                username: "u".into(),
                database: "db".into(),
                has_password: false,
                ssl_mode: String::new(),
                env_label: None,
                created_at: String::new(),
                updated_at: String::new(),
                test_succeeded_at: None,
                test_failed_at: None,
            }],
            cursor: 0,
            scroll: 0,
            scroll_locked: false,
            restore_cursor: None,
            form: None,
            status: None,
            status_kind:
                crate::features::instance_workspace::connections::state::ConnectionStatusKind::Idle,
            test_cooldown_until: None,
        };
        let msg = key_to_msg(key(KeyCode::Char('d'), KeyModifiers::NONE), &state)
            .expect("d should open the delete-confirm modal");
        match msg {
            AppMsg::OpenModal(ModalKind::DeleteConnectionConfirm {
                instance,
                connection,
            }) => {
                assert_eq!(instance, "inst");
                assert_eq!(connection, "conn");
            }
            other => panic!("expected DeleteConnectionConfirm modal, got {other:?}"),
        }
    }

    // The overview/connections per-pane key mapping (j/k/r/t/dd/cooldowns)
    // lives in the instance-workspace feature's `input` module and is tested
    // there. This app-level test only pins the shell routing: a plain key on
    // the overview pane is forwarded into the feature and wrapped as an `Iw`
    // message.
    #[test]
    fn instance_workspace_key_routes_to_feature_message() {
        use crate::features::instance_workspace::overview::msg::{OverviewMessage, OverviewMsg};
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::InstanceWorkspace(IwPane::Overview);
        state.iw.instance_name = "inst".to_string();
        let msg = key_to_msg(key(KeyCode::Char('j'), KeyModifiers::NONE), &state)
            .expect("j in overview should route into the iw feature");
        assert!(matches!(
            msg,
            AppMsg::Iw(IwMsg::Message(IwMessage::Overview(OverviewMsg::Message(
                OverviewMessage::MoveCursor(1)
            ))))
        ));
    }
}
