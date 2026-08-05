//! History feature messages.

use crossterm::event::KeyEvent;

/// The actual history messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryMessage {
    /// Record a successful statement for the connection.
    RecordSuccess { instance: String, connection: String, sql: String },
    /// Move the list cursor by `delta` (`-1`/`+1`).
    MoveCursor { delta: i32 },
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
