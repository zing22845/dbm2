//! History feature messages.

use crossterm::event::KeyEvent;

/// The actual history messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryMessage {
    /// Record a successful statement for the connection.
    RecordSuccess { instance: String, connection: String, sql: String },
    /// Move the list cursor by `delta` (`-1`/`+1`).
    MoveCursor { delta: i32 },
    /// Set the list cursor to an absolute index (from a mouse click).
    SetCursor { index: usize },
    /// Begin `/` search input.
    BeginSearch,
    /// Forward a key while search input is active.
    SearchKey(KeyEvent),
    /// Apply the selected entry (emits a `Recall` intent).
    Apply,
    /// Scroll the detail preview by `delta` lines.
    ScrollDetail { delta: i32 },
    /// Scroll the detail preview by half a page.
    ScrollDetailPage { down: bool },
    /// Scroll the list rows horizontally by `delta` cells (`-1`/`+1`).
    ScrollHScroll { delta: i32 },
    /// Set the list horizontal scroll to an absolute position (from scrollbar drag).
    SetHScroll { position: usize },
    /// Set the list vertical scroll offset to an absolute position (from scrollbar drag).
    /// The position represents the viewport's first visible row index.
    SetVScroll { position: usize },
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryMsg {
    Message(HistoryMessage),
}

impl From<HistoryMessage> for HistoryMsg {
    fn from(m: HistoryMessage) -> Self {
        HistoryMsg::Message(m)
    }
}
