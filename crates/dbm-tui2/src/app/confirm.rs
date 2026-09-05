//! Resolve the `Yes` action of an open confirm dialog.
//!
//! Both input channels dispatch a confirmation when the user agrees: the
//! keyboard layer on `y`/`Y` (see [`crate::app::key`]) and the mouse layer when
//! the Yes button is clicked (see [`crate::app::mouse`]). The mapping from a
//! modal to the action that confirms it belongs to neither channel, so it lives
//! here at the `app` level, above both input trees.

use crate::app::msg::AppMsg;
use crate::app::state::{AppState, ModalKind};
use crate::features::instance_workspace::connections::msg::{ConnectionsMessage, ConnectionsMsg};
use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
use crate::features::sql_workspace::sql_tab::results::msg::{ResultsMessage as R, ResultsMsg};

/// The action dispatched when a confirm modal's `Yes`/`y` is triggered (by key
/// or by clicking the Yes button). The shell closes the modal itself when it
/// sees the dispatched message (e.g. `DeleteConnection` / `UnregisterInstance`),
/// or the action runner does (e.g. a `Commit`). `None` for non-confirm modals.
pub(crate) fn confirm_yes_msg(modal: &ModalKind, state: &AppState) -> Option<AppMsg> {
    match modal {
        // Confirm the commit: dispatch Commit to the active tab's results
        // (the modal closes when the commit completes, via `CommitResult`).
        ModalKind::ResultsEditCommitPreview { .. } => state
            .sql
            .sql_tab
            .active_tab()
            .map(|t| t.session.id)
            .map(|id| {
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Results {
                        tab_id: id,
                        msg: ResultsMsg::Message(R::Commit),
                    },
                ))))
            }),
        // Confirm deleting a connection: dispatch the delete to the connections
        // panel (the shell closes the modal when it sees DeleteConnection).
        ModalKind::DeleteConnectionConfirm {
            instance,
            connection,
        } => Some(AppMsg::Iw(IwMsg::Message(IwMessage::Connections(
            ConnectionsMsg::Message(ConnectionsMessage::DeleteConnection {
                instance_name: instance.clone(),
                connection_name: connection.clone(),
            }),
        )))),
        // Confirm unregistering the current instance: dispatch the unregister
        // (the shell closes the modal when it sees UnregisterInstance).
        ModalKind::UnregisterInstanceConfirm { instance } => {
            Some(AppMsg::Iw(IwMsg::Message(IwMessage::UnregisterInstance {
                instance: instance.clone(),
            })))
        }
        _ => None,
    }
}
