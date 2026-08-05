//! Keyboard input forwarding.
//!
//! The run loop reads raw key events; global shortcuts are handled there, and
//! everything else is handed to [`key_to_msg`], which maps a key to a feature
//! message *according to the current focus zone*. This keeps key parsing
//! centralised in one place (per feature) instead of leaking into each
//! feature's `update`.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app_shell::focus::FocusZone;

use super::msg::AppMsg;
use crate::features::header::msg::{HeaderMessage, HeaderMsg};

/// Map a key to a feature message given the current focus zone.
///
/// Returns `None` when the key is not consumed by the focused feature (the run
/// loop treats it as a no-op). Global shortcuts (quit, theme toggle) are
/// handled separately by the run loop and are not routed here.
///
/// Each focus zone is delegated to its own helper (`header_key`, and later
/// `explorer_key`, `sql_key`, ...). Keep `key_to_msg` a thin dispatcher and put
/// per-feature key parsing in those helpers so it stays readable as features
/// grow; split helpers into sub-modules if a single file becomes crowded.
pub fn key_to_msg(key: KeyEvent, focus: FocusZone) -> Option<AppMsg> {
    match focus {
        FocusZone::Header => header_key(key),
        // Features not yet migrated keep no key bindings; add arms here as
        // their interaction logic is ported.
        FocusZone::Explorer
        | FocusZone::SQLWorkspace
        | FocusZone::InstanceWorkspace => None,
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
