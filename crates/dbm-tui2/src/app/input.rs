//! Keyboard input forwarding.
//!
//! The run loop reads raw key events; global shortcuts are handled there, and
//! everything else is handed to [`key_to_msg`], which maps a key to a feature
//! message. When a modal is open it owns all keys; otherwise the key is routed
//! by the active focus zone. This keeps key parsing centralised in one place
//! (per feature) instead of leaking into each feature's `update`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app_shell::focus::FocusZone;
use crate::features::discover::msg::{DiscoverMessage, DiscoverMsg};
use crate::features::discover::results::msg::{ResultsMessage, ResultsMsg};
use crate::features::discover::state::{DiscoverFocus, DiscoverState};
use crate::features::discover::targets::msg::{TargetsMessage, TargetsMsg};
use crate::features::explorer::instances::msg::{InstancesMessage, InstancesMsg};
use crate::features::explorer::msg::{ExplorerMessage, ExplorerMsg};
use crate::features::explorer::objects::msg::{ObjectsMessage, ObjectsMsg};
use crate::features::explorer::state::{ExplorerPane, ExplorerState};
use crate::features::header::msg::{HeaderMessage, HeaderMsg};
use crate::features::instance_workspace::connections::msg::{ConnectionsMessage, ConnectionsMsg};
use crate::features::instance_workspace::msg::{IwMessage, IwMsg};
use crate::features::instance_workspace::state::{IwState};
use crate::features::instance_workspace::connections::state::FormField;
use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
use crate::features::sql_workspace::state::SqlState;
use crate::features::sql_workspace::sql_tab::editor::context_picker::state::PickerColumn;
use crate::features::sql_workspace::sql_tab::editor::context_picker::msg::{ContextPickerMessage, ContextPickerMsg};
use crate::features::sql_workspace::sql_tab::editor::msg::{EditorMessage, EditorMsg};
use crate::features::sql_workspace::sql_tab::editor::sql_completion::msg::{SqlCompletionMessage, SqlCompletionMsg};
use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};

use super::msg::AppMsg;
use super::state::ModalKind;

/// Map a key to a feature message. A modal (if open) consumes all keys;
/// otherwise the active focus zone routes the key.
///
/// Returns `None` when nothing consumed the key (a no-op). Global shortcuts
/// (quit, theme toggle) are handled by the run loop and not routed here.
pub fn key_to_msg(key: KeyEvent, state: &super::state::AppState) -> Option<AppMsg> {
    match &state.modal {
        Some(ModalKind::Discover) => discover_key(key, &state.discover),
        // Data-carrying popups: their key routing will be wired once each
        // popup's owning state is connected; for now only ESC to dismiss and
        // y/n to confirm are recognized.
        Some(modal) => modal_key(key, modal),
        None => match state.focus {
            FocusZone::Header => header_key(key),
            FocusZone::Explorer => explorer_key(key, &state.explorer),
            FocusZone::InstanceWorkspace => iw_key(key, &state.iw),
            FocusZone::SQLWorkspace => sql_key(key, &state.sql),
        },
    }
}

/// Keys for the data-carrying popups. Returns `Some` only when the popup has
/// an active action to take; picker/page inputs are no-ops until wired.
fn modal_key(_key: KeyEvent, _modal: &ModalKind) -> Option<AppMsg> {
    None
}

/// Key bindings for the Header focus zone: move the button cursor and activate.
fn header_key(key: KeyEvent) -> Option<AppMsg> {
    let msg = match key.code {
        KeyCode::Left => HeaderMessage::MoveLeft,
        KeyCode::Right => HeaderMessage::MoveRight,
        KeyCode::Enter => HeaderMessage::Activate,
        _ => return None,
    };
    Some(AppMsg::Header(HeaderMsg::Message(msg)))
}

/// Discover key bindings, dispatched by the close-confirm flag and the active
/// discover pane.
fn discover_key(key: KeyEvent, state: &DiscoverState) -> Option<AppMsg> {
    let code = key.code;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if state.close_confirm {
        return match code {
            KeyCode::Enter => Some(discover(DiscoverMessage::Close)),
            KeyCode::Esc => Some(discover(DiscoverMessage::CancelClose)),
            _ => None,
        };
    }

    // Pane-move chords (Ctrl+arrows / Ctrl+h/l) take precedence.
    if ctrl {
        return match code {
            KeyCode::Left | KeyCode::Char('h') => Some(discover(DiscoverMessage::Focus(prev_pane(state.focus)))),
            KeyCode::Right | KeyCode::Char('l') => Some(discover(DiscoverMessage::Focus(next_pane(state.focus)))),
            _ => None,
        };
    }

    match code {
        KeyCode::Esc => Some(discover(DiscoverMessage::RequestClose)),
        // Scan / register are discover-level actions available from any pane.
        KeyCode::Char('s') => Some(discover(DiscoverMessage::StartScan)),
        KeyCode::Char('r') => Some(discover(DiscoverMessage::RegisterSelected)),
        _ => match state.focus {
            DiscoverFocus::Engine => engine_pane_key(key),
            DiscoverFocus::Targets => targets_pane_key(key, state),
            DiscoverFocus::Results => results_pane_key(key),
        },
    }
}

fn engine_pane_key(key: KeyEvent) -> Option<AppMsg> {
    // Engine is the only available engine; Enter re-focuses it (a no-op). We
    // consume it so it does not fall through to other handlers.
    match key.code {
        KeyCode::Enter | KeyCode::Char('e') => Some(discover(DiscoverMessage::Focus(DiscoverFocus::Engine))),
        _ => None,
    }
}

fn targets_pane_key(key: KeyEvent, state: &DiscoverState) -> Option<AppMsg> {
    let code = key.code;
    if state.targets.editing {
        return match code {
            KeyCode::Esc => Some(targets(TargetsMessage::CancelEdit)),
            KeyCode::Enter => Some(targets(TargetsMessage::CommitEdit)),
            KeyCode::Backspace => Some(targets(TargetsMessage::EditBackspace)),
            KeyCode::Left => Some(targets(TargetsMessage::EditCursorLeft)),
            KeyCode::Right => Some(targets(TargetsMessage::EditCursorRight)),
            KeyCode::Char(c) if !c.is_control() => Some(targets(TargetsMessage::EditChar(c))),
            _ => None,
        };
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match code {
        KeyCode::Char('i') | KeyCode::Enter => Some(targets(TargetsMessage::BeginEdit)),
        KeyCode::Char('o') => Some(targets(TargetsMessage::AddRow)),
        KeyCode::Char('d') | KeyCode::Delete => Some(targets(TargetsMessage::DeleteRow)),
        KeyCode::Char('u') => Some(targets(TargetsMessage::Undo)),
        KeyCode::Char('r') if ctrl => Some(targets(TargetsMessage::Redo)),
        KeyCode::Up | KeyCode::Char('k') => Some(targets(TargetsMessage::MoveUp)),
        KeyCode::Down | KeyCode::Char('j') => Some(targets(TargetsMessage::MoveDown)),
        KeyCode::Left | KeyCode::Char('h') => Some(targets(TargetsMessage::MoveColHost)),
        KeyCode::Right | KeyCode::Char('l') => Some(targets(TargetsMessage::MoveColPorts)),
        _ => None,
    }
}

fn results_pane_key(key: KeyEvent) -> Option<AppMsg> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(results(ResultsMessage::MoveUp)),
        KeyCode::Down | KeyCode::Char('j') => Some(results(ResultsMessage::MoveDown)),
        KeyCode::Char(' ') => Some(results(ResultsMessage::ToggleSelect)),
        KeyCode::Char('u') => Some(results(ResultsMessage::ToggleUnregisteredFilter)),
        _ => None,
    }
}

fn prev_pane(focus: DiscoverFocus) -> DiscoverFocus {
    match focus {
        DiscoverFocus::Engine => DiscoverFocus::Results,
        DiscoverFocus::Targets => DiscoverFocus::Engine,
        DiscoverFocus::Results => DiscoverFocus::Targets,
    }
}

fn next_pane(focus: DiscoverFocus) -> DiscoverFocus {
    match focus {
        DiscoverFocus::Engine => DiscoverFocus::Targets,
        DiscoverFocus::Targets => DiscoverFocus::Results,
        DiscoverFocus::Results => DiscoverFocus::Engine,
    }
}

fn discover(msg: DiscoverMessage) -> AppMsg {
    AppMsg::Discover(DiscoverMsg::Message(msg))
}
fn targets(msg: TargetsMessage) -> AppMsg {
    AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Targets(TargetsMsg::Message(msg))))
}
fn results(msg: ResultsMessage) -> AppMsg {
    AppMsg::Discover(DiscoverMsg::Message(DiscoverMessage::Results(
        ResultsMsg::Message(msg),
    )))
}

/// Explorer key bindings, dispatched by the active explorer pane.
fn explorer_key(key: KeyEvent, state: &ExplorerState) -> Option<AppMsg> {
    // `Tab` toggles between the instances and objects panes.
    if key.code == KeyCode::Tab {
        let next = match state.pane {
            ExplorerPane::Instances => ExplorerPane::Objects,
            ExplorerPane::Objects => ExplorerPane::Instances,
        };
        return Some(explorer(ExplorerMessage::SetPane(next)));
    }
    match state.pane {
        ExplorerPane::Instances => instances_key(key),
        ExplorerPane::Objects => objects_key(key),
    }
}

/// Objects pane keys: navigate the object tree.
fn objects_key(key: KeyEvent) -> Option<AppMsg> {
    let msg = match key.code {
        KeyCode::Up | KeyCode::Char('k') => ObjectsMessage::MoveUp,
        KeyCode::Down | KeyCode::Char('j') => ObjectsMessage::MoveDown,
        KeyCode::Enter => ObjectsMessage::Select,
        KeyCode::Right | KeyCode::Left => ObjectsMessage::ToggleExpand,
        _ => return None,
    };
    Some(explorer(ExplorerMessage::Objects(ObjectsMsg::Message(msg))))
}

/// Instances pane keys: navigate the connection tree.
fn instances_key(key: KeyEvent) -> Option<AppMsg> {
    let msg = match key.code {
        KeyCode::Up | KeyCode::Char('k') => InstancesMessage::MoveUp,
        KeyCode::Down | KeyCode::Char('j') => InstancesMessage::MoveDown,
        KeyCode::Enter => InstancesMessage::Select,
        KeyCode::Right => InstancesMessage::ToggleExpand,
        KeyCode::Left => InstancesMessage::ToggleExpand,
        _ => return None,
    };
    Some(explorer(ExplorerMessage::Instances(InstancesMsg::Message(msg))))
}

fn explorer(msg: ExplorerMessage) -> AppMsg {
    AppMsg::Explorer(ExplorerMsg::Message(msg))
}

/// Instance workspace key bindings: navigate/edit connections, or edit the
/// form when one is open.
fn iw_key(key: KeyEvent, state: &IwState) -> Option<AppMsg> {
    if state.connections.form.is_some() {
        return iw_form_key(key);
    }
    let msg = match key.code {
        KeyCode::Up | KeyCode::Char('k') => ConnectionsMessage::MoveUp,
        KeyCode::Down | KeyCode::Char('j') => ConnectionsMessage::MoveDown,
        KeyCode::Char('a') => ConnectionsMessage::BeginAdd,
        KeyCode::Char('e') | KeyCode::Enter => ConnectionsMessage::BeginEdit,
        KeyCode::Char('d') | KeyCode::Delete => ConnectionsMessage::Delete,
        _ => return None,
    };
    Some(iw(IwMessage::Connections(ConnectionsMsg::Message(msg))))
}

/// Form keys when a connection form is open.
fn iw_form_key(key: KeyEvent) -> Option<AppMsg> {
    let msg = match key.code {
        KeyCode::Esc => ConnectionsMessage::CancelForm,
        KeyCode::Enter => ConnectionsMessage::CommitForm,
        KeyCode::Up => ConnectionsMessage::FormField(FormField::Name),
        KeyCode::Down => ConnectionsMessage::FormField(FormField::Password),
        KeyCode::Tab => ConnectionsMessage::FormField(FormField::Database),
        KeyCode::Char(c) if !c.is_control() => ConnectionsMessage::FormChar(c),
        KeyCode::Backspace => ConnectionsMessage::FormBackspace,
        _ => return None,
    };
    Some(iw(IwMessage::Connections(ConnectionsMsg::Message(msg))))
}

fn iw(msg: IwMessage) -> AppMsg {
    AppMsg::Iw(IwMsg::Message(msg))
}

/// SQL workspace key bindings, routed to the active tab's editor and its
/// overlays (context picker / completion popup).
fn sql_key(key: KeyEvent, state: &SqlState) -> Option<AppMsg> {
    let tab = state.sql_tab.tabs.get(state.sql_tab.active_tab)?;
    let tab_id = tab.session.id;
    let editor = &tab.editor;

    // The context picker, when open, owns all keys.
    if editor.context_picker.open {
        return sql_context_picker_key(key, tab_id);
    }

    // The completion popup handles selection/apply/close when open.
    if editor.sql_completion.is_open() {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Some(sql_editor(EditorMessage::SqlCompletion(SqlCompletionMsg::Message(
                    SqlCompletionMessage::MoveSelection { delta: -1 },
                )), tab_id));
            }
            KeyCode::Down | KeyCode::Char('j') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Some(sql_editor(EditorMessage::SqlCompletion(SqlCompletionMsg::Message(
                    SqlCompletionMessage::MoveSelection { delta: 1 },
                )), tab_id));
            }
            KeyCode::Enter => {
                return Some(sql_editor(EditorMessage::SqlCompletion(SqlCompletionMsg::Message(
                    SqlCompletionMessage::Apply,
                )), tab_id));
            }
            KeyCode::Esc => {
                return Some(sql_editor(EditorMessage::SqlCompletion(SqlCompletionMsg::Message(
                    SqlCompletionMessage::Close,
                )), tab_id));
            }
            _ => {}
        }
    }

    // Tab-bar / tab management keys (only when the popups are closed).
    if let Some(msg) = sql_tab_navigation_key(key, state) {
        return Some(msg);
    }

    // Ctrl+Enter runs the current editor SQL.
    if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(sql_editor(EditorMessage::Run, tab_id));
    }

    // Otherwise forward the key to the editor buffer.
    editor_key(key, tab_id)
}

/// Tab-bar navigation keys: switch / open / close tabs.
fn sql_tab_navigation_key(key: KeyEvent, state: &SqlState) -> Option<AppMsg> {
    use crate::features::sql_workspace::sql_tab::msg::SqlTabMessage;
    let count = state.sql_tab.tabs.len();
    if count == 0 {
        return None;
    }
    let active = state.sql_tab.active_tab;
    let tab_msg = match key.code {
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            SqlTabMessage::Tab((active + 1) % count)
        }
        KeyCode::BackTab if key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            SqlTabMessage::Tab((active + count - 1) % count)
        }
        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            SqlTabMessage::CloseTab(active)
        }
        KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            SqlTabMessage::OpenTab
        }
        _ => return None,
    };
    Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(tab_msg)))))
}

/// Keys for the context picker overlay (owns all keys while open).
fn sql_context_picker_key(key: KeyEvent, tab_id: usize) -> Option<AppMsg> {
    let msg = match key.code {
        KeyCode::Esc => ContextPickerMessage::Close,
        KeyCode::Tab => ContextPickerMessage::MoveColumn(PickerColumn::Schema),
        KeyCode::BackTab => ContextPickerMessage::MoveColumn(PickerColumn::Database),
        KeyCode::Enter => ContextPickerMessage::Apply,
        KeyCode::Up | KeyCode::Char('k') => ContextPickerMessage::MoveCursor { delta: -1 },
        KeyCode::Down | KeyCode::Char('j') => ContextPickerMessage::MoveCursor { delta: 1 },
        KeyCode::Left | KeyCode::Char('h') => ContextPickerMessage::MoveColumn(PickerColumn::Database),
        KeyCode::Right | KeyCode::Char('l') => ContextPickerMessage::MoveColumn(PickerColumn::Schema),
        KeyCode::Char('/') => ContextPickerMessage::BeginSearch,
        _ => return None,
    };
    Some(sql_editor(EditorMessage::ContextPicker(ContextPickerMsg::Message(msg)), tab_id))
}

/// Forward a key to the editor buffer (typing / navigation / modal commands).
fn editor_key(key: KeyEvent, tab_id: usize) -> Option<AppMsg> {
    Some(sql_editor(EditorMessage::KeyEvent { key, tracked_caps_lock: false }, tab_id))
}

/// Build an `AppMsg::Sql` message targeting the given tab's editor.
fn sql_editor(msg: EditorMessage, tab_id: usize) -> AppMsg {
    AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
        SqlTabMessage::Editor {
            tab_id,
            msg: EditorMsg::Message(msg),
        },
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sql_workspace::sql_tab::state::SqlTabState;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    /// A `SqlState` with `count` tabs open. The default state already opens one
    /// tab, so we open `count.saturating_sub(1)` more on top of it.
    fn state_with_tabs(count: usize) -> SqlState {
        let mut tab_state = SqlTabState::default();
        for i in 1..count {
            tab_state.open_connection_tab(
                "local".into(),
                format!("conn-{i}"),
                format!("c{i}"),
                None,
                None,
            );
        }
        SqlState {
            sql_tab: tab_state,
        }
    }

    fn extract_tab_msg(msg: AppMsg) -> SqlTabMessage {
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(m)))) => m,
            _ => panic!("expected Sql tab message"),
        }
    }

    #[test]
    fn ctrl_tab_switches_to_next_tab() {
        let state = state_with_tabs(3); // active_tab = 2 (last opened)
        let msg = sql_tab_navigation_key(key(KeyCode::Tab, KeyModifiers::CONTROL), &state)
            .expect("ctrl+tab should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Tab(0));
    }

    #[test]
    fn ctrl_shift_tab_wraps_to_previous_tab() {
        let state = state_with_tabs(2); // active_tab = 1
        let msg = sql_tab_navigation_key(
            key(KeyCode::BackTab, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
            &state,
        )
        .expect("ctrl+shift+tab should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Tab(0));
    }

    #[test]
    fn ctrl_w_closes_active_tab() {
        let state = state_with_tabs(2);
        let msg = sql_tab_navigation_key(key(KeyCode::Char('w'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+w should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::CloseTab(1));
    }

    #[test]
    fn single_tab_switching_wraps_to_itself() {
        let state = state_with_tabs(1);
        let msg = sql_tab_navigation_key(key(KeyCode::Tab, KeyModifiers::CONTROL), &state)
            .expect("ctrl+tab should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Tab(0));
    }
}
