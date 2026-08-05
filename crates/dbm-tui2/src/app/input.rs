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
use crate::features::explorer::state::{ExplorerPane, ExplorerState};
use crate::features::header::msg::{HeaderMessage, HeaderMsg};

use super::msg::AppMsg;
use super::state::ModalKind;

/// Map a key to a feature message. A modal (if open) consumes all keys;
/// otherwise the active focus zone routes the key.
///
/// Returns `None` when nothing consumed the key (a no-op). Global shortcuts
/// (quit, theme toggle) are handled by the run loop and not routed here.
pub fn key_to_msg(key: KeyEvent, state: &super::state::AppState) -> Option<AppMsg> {
    match state.modal {
        Some(ModalKind::Discover) => discover_key(key, &state.discover),
        None => match state.focus {
            FocusZone::Header => header_key(key),
            FocusZone::Explorer => explorer_key(key, &state.explorer),
            // Features not yet migrated keep no key bindings; add arms here as
            // their interaction logic is ported.
            FocusZone::SQLWorkspace | FocusZone::InstanceWorkspace => None,
        },
    }
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
    match state.pane {
        ExplorerPane::Instances => instances_key(key),
        ExplorerPane::Objects => None,
    }
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
