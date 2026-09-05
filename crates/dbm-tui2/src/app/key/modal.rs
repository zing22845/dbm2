//! Keys for the data-carrying popups (confirm dialogs, row-limit picker,
//! page input, commit preview) plus the shared confirm Yes/No logic.
//!
//! Part of the split keyboard layer; the entry points are
//! re-exported from the parent [`super`] module.

use super::sql::sql_results;
use crate::app::confirm::confirm_yes_msg;
use crate::app::msg::AppMsg;
use crate::app::state::{AppState, ModalKind};
use crossterm::event::{KeyCode, KeyEvent};

/// Shared key handling for a Yes/No confirm dialog: `y`/`Y` confirms (running
/// `on_yes`), `n`/`N` cancels (running `on_no`), anything else is unhandled.
/// Both the app-level confirm modals and the discover close-confirmation route
/// their keys through this so the confirm shortcut is defined in one place.
pub(super) fn confirm_yes_no_key(
    key: KeyEvent,
    on_yes: impl FnOnce() -> Option<AppMsg>,
    on_no: impl FnOnce() -> Option<AppMsg>,
) -> Option<AppMsg> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => on_yes(),
        KeyCode::Char('n') | KeyCode::Char('N') => on_no(),
        _ => None,
    }
}

/// Keys for the data-carrying popups (row-limit picker / page input / confirm
/// / commit preview). `Esc` closes; `n`/`N` cancels a confirm; `y`/`Y`/`Enter`
/// confirms and dispatches the owning feature's action (e.g. the commit preview
/// issues a `Commit`). All changes flow through `update` messages.
pub(super) fn modal_key(key: KeyEvent, modal: &ModalKind, state: &AppState) -> Option<AppMsg> {
    use crate::features::sql_workspace::sql_tab::results::msg::ResultsMessage as R;
    use ModalKind;
    let close = || AppMsg::CloseModal;
    let active_tab_id = || state.sql.sql_tab.active_tab().map(|t| t.session.id);
    match key.code {
        KeyCode::Esc => Some(close()),
        // Row-limit picker: up/down cycle the presets, enter applies.
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j') | KeyCode::Enter => {
            if let ModalKind::ResultsRowLimitPicker { current, limits } = modal {
                if key.code == KeyCode::Enter {
                    return active_tab_id()
                        .map(|id| sql_results(R::SetRowLimit { limit: *current }, id));
                }
                let delta = if matches!(key.code, KeyCode::Up | KeyCode::Char('k')) {
                    usize::MAX // -1 wraps below
                } else {
                    1usize
                };
                let idx = limits.iter().position(|l| *l == *current).unwrap_or(0);
                let len = limits.len().max(1);
                let next = if delta == 1 {
                    (idx + 1) % len
                } else {
                    (idx + len - 1) % len
                };
                return Some(AppMsg::OpenModal(ModalKind::ResultsRowLimitPicker {
                    current: limits[next],
                    limits: limits.clone(),
                }));
            }
            // Page input: enter applies the shown page.
            if let ModalKind::ResultsPageInput { current_page, .. } = modal {
                return active_tab_id().map(|id| {
                    sql_results(
                        R::SetPage {
                            page: *current_page,
                        },
                        id,
                    )
                });
            }
            None
        }
        // Confirm modals: `y`/`Y` confirms, `n`/`N` cancels (shared handling).
        _ => confirm_yes_no_key(
            key,
            || confirm_yes_msg(modal, state),
            || {
                if crate::common::view::modal::is_confirm_modal(modal) {
                    Some(close())
                } else {
                    None
                }
            },
        ),
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }
    #[test]
    fn modal_y_confirms_delete_connection() {
        use crate::app::state::ModalKind;
        use crate::features::instance_workspace::connections::msg::{
            ConnectionsMessage, ConnectionsMsg,
        };
        use crate::features::instance_workspace::msg::{IwMessage, IwMsg};

        let state = crate::app::state::AppState::default();
        let modal = ModalKind::DeleteConnectionConfirm {
            instance: "inst".into(),
            connection: "conn".into(),
        };
        let msg = modal_key(key(KeyCode::Char('y'), KeyModifiers::NONE), &modal, &state)
            .expect("y should confirm delete");
        match msg {
            AppMsg::Iw(IwMsg::Message(IwMessage::Connections(ConnectionsMsg::Message(
                ConnectionsMessage::DeleteConnection {
                    instance_name,
                    connection_name,
                },
            )))) => {
                assert_eq!(instance_name, "inst");
                assert_eq!(connection_name, "conn");
            }
            other => panic!("expected DeleteConnection, got {other:?}"),
        }
    }
}
