//! History list sub-feature messages.

use crossterm::event::KeyEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListMessage {
    /// Move the list cursor by `delta` (`-1`/`+1`).
    MoveCursor { delta: i32 },
    /// Set the list cursor to an absolute visible row index (from a mouse click).
    SetCursor { index: usize },
    /// Begin `/` search input.
    BeginSearch,
    /// Forward a key while search input is active.
    SearchKey(KeyEvent),
    /// Scroll the list rows horizontally by `delta` cells (`-1`/`+1`).
    ScrollHScroll { delta: i32 },
    /// Set the list horizontal scroll to an absolute position (from scrollbar drag).
    SetHScroll { position: usize },
    /// Set the list vertical scroll offset to an absolute position (from scrollbar drag).
    /// The position represents the viewport's first visible row index.
    SetVScroll { position: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListMsg {
    Message(ListMessage),
}

impl From<ListMessage> for ListMsg {
    fn from(m: ListMessage) -> Self {
        ListMsg::Message(m)
    }
}
