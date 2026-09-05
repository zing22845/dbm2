//! Instance workspace keyboard input.
//!
//! Maps keys to feature messages for the instance workspace, reading the
//! feature's own state to decide what a key does (e.g. the overview's `r`
//! honors its refresh cooldown, the connections form's `dd` clear-field chord
//! tracks its pending-`d` timestamp). The app shell keeps only the shell-level
//! `Tab` pane navigation and the modal-data conversion; everything that touches
//! feature state lives here.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app_shell::nav::IwPane;

use super::connections::msg::{ConnectionsMessage, ConnectionsMsg};
use super::connections::state::FormMode;
use super::msg::IwMessage;
use super::overview::msg::{OverviewMessage, OverviewMsg};
use super::state::IwState;

/// The result of mapping a key in the instance workspace: either a feature
/// message to dispatch, or a request for the app shell to open one of the
/// confirm modals (the shell owns the modal and its `ModalKind`).
#[derive(Debug, Clone)]
pub enum IwInput {
    /// A feature message to dispatch into the instance workspace's update.
    Message(IwMessage),
    /// Open the "unregister instance" confirm modal (the overview's `u`). The
    /// instance name is filled in here from the feature's own state so the
    /// shell does not need to read it.
    OpenUnregisterConfirm { instance: String },
    /// Open the "delete connection" confirm modal (the connections pane's `d`).
    /// The instance and selected connection names are filled in here.
    OpenDeleteConfirm {
        instance: String,
        connection: String,
    },
}

/// Map a key to an instance-workspace action while the workspace is focused.
/// `pane` is the active sub-pane (overview / connections); the routing
/// decisions (form open, per-pane keys, cooldowns) read `state` directly.
///
/// Returns `None` when nothing consumed the key. The `Tab` pane-navigation and
/// Ctrl+nav are handled by the shell, not here.
pub fn key_to_msg(key: KeyEvent, pane: IwPane, state: &IwState) -> Option<IwInput> {
    // A connection form, when open, owns all keys.
    if state.connections.form.is_some() {
        return iw_form_key(key, state);
    }
    match pane {
        IwPane::Overview => overview_key(key, state),
        IwPane::Connections => connections_key(key, state),
    }
}

/// Overview panel keys: move the cursor (j/k, ↑/↓), H-Scroll (←/→), refresh
/// (`r`), and unregister (`u`) which requests a confirm modal.
fn overview_key(key: KeyEvent, state: &IwState) -> Option<IwInput> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(overview(OverviewMessage::MoveCursor(-1))),
        KeyCode::Down | KeyCode::Char('j') => Some(overview(OverviewMessage::MoveCursor(1))),
        KeyCode::Char('u') => {
            if state.instance_name.is_empty() {
                None
            } else {
                Some(IwInput::OpenUnregisterConfirm {
                    instance: state.instance_name.clone(),
                })
            }
        }
        KeyCode::Char('r') => {
            // Refresh the open instance (matching the original dbm): a 1s
            // cooldown set on refresh means a held `r` fires once and ignores
            // the auto-repeat.
            if state.instance_name.is_empty()
                || state
                    .overview
                    .refresh_cooldown_until
                    .is_some_and(|until| std::time::Instant::now() < until)
            {
                None
            } else {
                Some(IwInput::Message(IwMessage::Refresh {
                    instance_name: state.instance_name.clone(),
                }))
            }
        }
        _ => None,
    }
}

/// Connections panel keys: navigate the list (j/k, ↑/↓), add (`a`), edit
/// (`i`), test (`t`), and delete (`d`) which requests a confirm modal.
fn connections_key(key: KeyEvent, state: &IwState) -> Option<IwInput> {
    // `d` opens a delete-confirm modal for the selected connection (matching
    // the original dbm); the actual delete is dispatched from the modal's `y`.
    if matches!(key.code, KeyCode::Char('d') | KeyCode::Delete) {
        let connection = state.connections.selected_name()?;
        return Some(IwInput::OpenDeleteConfirm {
            instance: state.instance_name.clone(),
            connection,
        });
    }
    let msg = match key.code {
        KeyCode::Up | KeyCode::Char('k') => ConnectionsMessage::MoveUp,
        KeyCode::Down | KeyCode::Char('j') => ConnectionsMessage::MoveDown,
        // Align with the original dbm: `a` adds, `i` edits the selected
        // connection (no separate `e`/`Enter` binding).
        KeyCode::Char('a') => ConnectionsMessage::BeginAdd,
        KeyCode::Char('i') => ConnectionsMessage::BeginEdit,
        // Test the selected connection (the list's `t`), matching dbm, at most
        // once per second (cooldown set in the update).
        KeyCode::Char('t') => {
            if state
                .connections
                .test_cooldown_until
                .is_some_and(|until| std::time::Instant::now() < until)
            {
                return None;
            }
            ConnectionsMessage::TestSelected
        }
        _ => return None,
    };
    Some(connections(msg))
}

/// Form keys when a connection form is open, matching the original dbm: typing
/// happens in an explicit per-field insert mode. In insert mode keys edit the
/// current field (`Enter` commits it, `Esc` reverts it); in normal mode `i`
/// starts editing a field, `j`/`k` move between fields, `Enter` saves the whole
/// connection and `Esc` cancels the form.
fn iw_form_key(key: KeyEvent, state: &IwState) -> Option<IwInput> {
    let form = state.connections.form.as_ref()?;
    let insert = form.mode == FormMode::Insert;
    let msg = if insert {
        match key.code {
            // In insert mode, printable characters go straight into the field.
            KeyCode::Char(c) if !c.is_control() => ConnectionsMessage::FormChar(c),
            KeyCode::Backspace => ConnectionsMessage::FormBackspace,
            KeyCode::Enter => ConnectionsMessage::CommitFieldInsert,
            KeyCode::Esc => ConnectionsMessage::CancelFieldInsert,
            _ => return None,
        }
    } else {
        match key.code {
            KeyCode::Char('i') | KeyCode::Char('I') => ConnectionsMessage::BeginFieldInsert,
            KeyCode::Up | KeyCode::Char('k') => ConnectionsMessage::FormField(form.field.prev()),
            KeyCode::Down | KeyCode::Char('j') => ConnectionsMessage::FormField(form.field.next()),
            KeyCode::Enter => ConnectionsMessage::CommitForm,
            KeyCode::Esc => ConnectionsMessage::CancelForm,
            // Test the form's current values against the database, at most once
            // per second (cooldown set in the update).
            KeyCode::Char('t') | KeyCode::Char('T') => {
                if state
                    .connections
                    .test_cooldown_until
                    .is_some_and(|until| std::time::Instant::now() < until)
                {
                    return None;
                }
                ConnectionsMessage::TestForm
            }
            // `dd` clears the current field and enters insert mode: the first
            // `d` arms a short window, a second `d` within it clears.
            KeyCode::Char('d') => {
                let within = form
                    .pending_d_at
                    .is_some_and(|at| at.elapsed() <= std::time::Duration::from_millis(300));
                if within {
                    ConnectionsMessage::ClearFieldAndInsert
                } else {
                    ConnectionsMessage::SetPendingD
                }
            }
            _ => return None,
        }
    };
    Some(connections(msg))
}

/// Wrap an overview message as an `IwInput::Message`.
fn overview(msg: OverviewMessage) -> IwInput {
    IwInput::Message(IwMessage::Overview(OverviewMsg::Message(msg)))
}

/// Wrap a connections message as an `IwInput::Message`.
fn connections(msg: ConnectionsMessage) -> IwInput {
    IwInput::Message(IwMessage::Connections(ConnectionsMsg::Message(msg)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn state_with(pane: IwPane, instance: &str) -> IwState {
        let mut s = IwState::default();
        s.pane = pane;
        s.instance_name = instance.into();
        s
    }

    #[test]
    fn overview_jk_moves_cursor() {
        let s = state_with(IwPane::Overview, "inst");
        assert!(matches!(
            key_to_msg(key(KeyCode::Char('j')), IwPane::Overview, &s),
            Some(IwInput::Message(IwMessage::Overview(OverviewMsg::Message(
                OverviewMessage::MoveCursor(1)
            ))))
        ));
        assert!(matches!(
            key_to_msg(key(KeyCode::Char('k')), IwPane::Overview, &s),
            Some(IwInput::Message(IwMessage::Overview(OverviewMsg::Message(
                OverviewMessage::MoveCursor(-1)
            ))))
        ));
    }

    #[test]
    fn overview_r_refreshes_instance() {
        let s = state_with(IwPane::Overview, "inst-a");
        assert!(matches!(
            key_to_msg(key(KeyCode::Char('r')), IwPane::Overview, &s),
            Some(IwInput::Message(IwMessage::Refresh { instance_name }))
                if instance_name == "inst-a"
        ));
    }

    #[test]
    fn overview_r_honors_refresh_cooldown() {
        let mut s = state_with(IwPane::Overview, "inst-a");
        s.overview.refresh_cooldown_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
        assert!(key_to_msg(key(KeyCode::Char('r')), IwPane::Overview, &s).is_none());
        s.overview.refresh_cooldown_until =
            Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
        assert!(key_to_msg(key(KeyCode::Char('r')), IwPane::Overview, &s).is_some());
    }

    #[test]
    fn overview_u_requests_unregister_confirm() {
        let s = state_with(IwPane::Overview, "inst-a");
        assert!(matches!(
            key_to_msg(key(KeyCode::Char('u')), IwPane::Overview, &s),
            Some(IwInput::OpenUnregisterConfirm { instance }) if instance == "inst-a"
        ));
    }

    #[test]
    fn overview_u_ignored_without_instance() {
        let s = state_with(IwPane::Overview, "");
        assert!(key_to_msg(key(KeyCode::Char('u')), IwPane::Overview, &s).is_none());
    }

    #[test]
    fn connections_navigation() {
        let s = state_with(IwPane::Connections, "inst");
        assert!(matches!(
            key_to_msg(key(KeyCode::Char('j')), IwPane::Connections, &s),
            Some(IwInput::Message(IwMessage::Connections(
                ConnectionsMsg::Message(ConnectionsMessage::MoveDown)
            )))
        ));
        assert!(matches!(
            key_to_msg(key(KeyCode::Char('k')), IwPane::Connections, &s),
            Some(IwInput::Message(IwMessage::Connections(
                ConnectionsMsg::Message(ConnectionsMessage::MoveUp)
            )))
        ));
    }

    #[test]
    fn connections_add_edit() {
        let s = state_with(IwPane::Connections, "inst");
        assert!(matches!(
            key_to_msg(key(KeyCode::Char('a')), IwPane::Connections, &s),
            Some(IwInput::Message(IwMessage::Connections(
                ConnectionsMsg::Message(ConnectionsMessage::BeginAdd)
            )))
        ));
        assert!(matches!(
            key_to_msg(key(KeyCode::Char('i')), IwPane::Connections, &s),
            Some(IwInput::Message(IwMessage::Connections(
                ConnectionsMsg::Message(ConnectionsMessage::BeginEdit)
            )))
        ));
    }

    #[test]
    fn connections_t_honors_test_cooldown() {
        let mut s = state_with(IwPane::Connections, "inst");
        s.connections.test_cooldown_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
        assert!(key_to_msg(key(KeyCode::Char('t')), IwPane::Connections, &s).is_none());
        s.connections.test_cooldown_until =
            Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
        assert!(matches!(
            key_to_msg(key(KeyCode::Char('t')), IwPane::Connections, &s),
            Some(IwInput::Message(IwMessage::Connections(
                ConnectionsMsg::Message(ConnectionsMessage::TestSelected)
            )))
        ));
    }

    #[test]
    fn connections_d_requests_delete_confirm() {
        let mut s = state_with(IwPane::Connections, "inst");
        s.connections.connections = vec![dbm_store::InstanceConnection {
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
        }];
        assert!(matches!(
            key_to_msg(key(KeyCode::Char('d')), IwPane::Connections, &s),
            Some(IwInput::OpenDeleteConfirm { instance, connection })
                if instance == "inst" && connection == "conn"
        ));
    }

    #[test]
    fn connections_d_ignored_without_selection() {
        let s = state_with(IwPane::Connections, "inst");
        assert!(key_to_msg(key(KeyCode::Char('d')), IwPane::Connections, &s).is_none());
    }

    #[test]
    fn form_insert_mode_edits_field() {
        let mut s = state_with(IwPane::Connections, "inst");
        s.connections.form = Some(super::super::connections::state::ConnectionForm {
            name: "n".into(),
            field: super::super::connections::state::FormField::Name,
            mode: FormMode::Insert,
            ..Default::default()
        });
        assert!(matches!(
            key_to_msg(key(KeyCode::Char('x')), IwPane::Connections, &s),
            Some(IwInput::Message(IwMessage::Connections(
                ConnectionsMsg::Message(ConnectionsMessage::FormChar('x'))
            )))
        ));
    }

    #[test]
    fn form_normal_mode_dd_clears_field() {
        let mut s = state_with(IwPane::Connections, "inst");
        s.connections.form = Some(super::super::connections::state::ConnectionForm {
            name: "n".into(),
            field: super::super::connections::state::FormField::Name,
            mode: FormMode::Normal,
            ..Default::default()
        });
        // First `d` arms the pending-d flag.
        assert!(matches!(
            key_to_msg(key(KeyCode::Char('d')), IwPane::Connections, &s),
            Some(IwInput::Message(IwMessage::Connections(
                ConnectionsMsg::Message(ConnectionsMessage::SetPendingD)
            )))
        ));
        // A second `d` within the window clears the field and enters insert.
        s.connections.form.as_mut().unwrap().pending_d_at = Some(std::time::Instant::now());
        assert!(matches!(
            key_to_msg(key(KeyCode::Char('d')), IwPane::Connections, &s),
            Some(IwInput::Message(IwMessage::Connections(
                ConnectionsMsg::Message(ConnectionsMessage::ClearFieldAndInsert)
            )))
        ));
    }
}
