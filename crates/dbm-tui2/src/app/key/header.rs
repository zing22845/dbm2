//! Key bindings for the Header focus pane.
//!
//! Part of the split keyboard layer; the entry points are
//! re-exported from the parent [`super`] module.

use crate::app::msg::AppMsg;
use crate::features::header::msg::{HeaderMessage, HeaderMsg};
use crossterm::event::{KeyCode, KeyEvent};
/// Key bindings for the Header focus pane: move the button cursor and activate.
pub(crate) fn header_key(key: KeyEvent) -> Option<AppMsg> {
    let msg = match key.code {
        KeyCode::Left => HeaderMessage::MoveLeft,
        KeyCode::Right => HeaderMessage::MoveRight,
        KeyCode::Enter => HeaderMessage::Activate,
        _ => return None,
    };
    Some(AppMsg::Header(HeaderMsg::Message(msg)))
}
