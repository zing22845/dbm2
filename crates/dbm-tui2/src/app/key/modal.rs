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
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j') => {
            if let ModalKind::ResultsRowLimitPicker { current, limits } = modal {
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
            None
        }
        // Page input: digits build the target, Backspace edits, Enter jumps.
        KeyCode::Char(c)
            if c.is_ascii_digit() && matches!(modal, ModalKind::ResultsPageInput { .. }) =>
        {
            if let ModalKind::ResultsPageInput {
                current_page,
                total_pages,
                input,
            } = modal
            {
                let mut next = input.clone();
                if next.len() < 8 {
                    next.push(c);
                }
                return Some(AppMsg::OpenModal(ModalKind::ResultsPageInput {
                    current_page: *current_page,
                    total_pages: *total_pages,
                    input: next,
                }));
            }
            None
        }
        KeyCode::Backspace if matches!(modal, ModalKind::ResultsPageInput { .. }) => {
            if let ModalKind::ResultsPageInput {
                current_page,
                total_pages,
                input,
            } = modal
            {
                let mut next = input.clone();
                next.pop();
                return Some(AppMsg::OpenModal(ModalKind::ResultsPageInput {
                    current_page: *current_page,
                    total_pages: *total_pages,
                    input: next,
                }));
            }
            None
        }
        KeyCode::Enter => match modal {
            ModalKind::ResultsRowLimitPicker { current, .. } => {
                active_tab_id().map(|id| sql_results(R::SetRowLimit { limit: *current }, id))
            }
            ModalKind::ResultsPageInput {
                current_page,
                total_pages,
                input,
            } => {
                // Resolve the typed page (clamped to the known page count,
                // falling back to the current page for empty/invalid input).
                let parsed = input.trim().parse::<usize>().unwrap_or(*current_page);
                let page = if parsed == 0 {
                    *current_page
                } else {
                    match total_pages {
                        Some(max) => parsed.min((*max).max(1)),
                        None => parsed,
                    }
                };
                active_tab_id().map(|id| sql_results(R::SetPage { page }, id))
            }
            // Confirm modals ignore Enter (they confirm with y / cancel with n).
            _ => None,
        },
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
    #[test]
    fn page_input_digit_appends_to_buffer_and_enter_jumps_clamped() {
        use crate::app::state::{AppState, ModalKind};
        use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
        use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
        use crate::features::sql_workspace::sql_tab::results::msg::{
            ResultsMessage as R, ResultsMsg,
        };

        let mut state = AppState::default();
        state.sql.sql_tab.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        let modal = ModalKind::ResultsPageInput {
            current_page: 2,
            total_pages: Some(9),
            input: "2".into(),
        };
        // A digit appends to the live buffer (keeping the page context).
        let msg = modal_key(key(KeyCode::Char('5'), KeyModifiers::NONE), &modal, &state)
            .expect("a digit should edit the page buffer");
        let buffer = match msg {
            AppMsg::OpenModal(ModalKind::ResultsPageInput { input, .. }) => input,
            other => panic!("expected an updated page-input modal, got {other:?}"),
        };
        assert_eq!(buffer, "25");

        // Enter applies the typed page, clamped to the total page count.
        let modal = ModalKind::ResultsPageInput {
            current_page: 2,
            total_pages: Some(9),
            input: buffer,
        };
        let msg = modal_key(key(KeyCode::Enter, KeyModifiers::NONE), &modal, &state)
            .expect("Enter should jump to the typed page");
        let page = match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Results {
                    msg: ResultsMsg::Message(R::SetPage { page }),
                    ..
                },
            )))) => page,
            other => panic!("expected Results SetPage, got {other:?}"),
        };
        assert_eq!(page, 9, "page 25 must clamp to the 9 available pages");
    }
    #[test]
    fn row_limit_picker_enter_applies_current_limit() {
        use crate::app::state::{AppState, ModalKind};
        use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
        use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
        use crate::features::sql_workspace::sql_tab::results::msg::{
            ResultsMessage as R, ResultsMsg,
        };

        let mut state = AppState::default();
        state.sql.sql_tab.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        let modal = ModalKind::ResultsRowLimitPicker {
            current: 200,
            limits: vec![50, 100, 200],
        };
        let msg = modal_key(key(KeyCode::Enter, KeyModifiers::NONE), &modal, &state)
            .expect("Enter should apply the selected row limit");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Results {
                    msg: ResultsMsg::Message(R::SetRowLimit { limit }),
                    ..
                },
            )))) => assert_eq!(limit, 200),
            other => panic!("expected Results SetRowLimit, got {other:?}"),
        }
    }
}
