//! Discovery results feature messages.

/// The actual results list messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsMessage {
    /// Move the cursor up.
    MoveUp,
    /// Move the cursor down.
    MoveDown,
    /// Toggle selection of the row under the cursor.
    ToggleSelect,
    /// Toggle the unregistered-only filter.
    ToggleUnregisteredFilter,
    /// Programmatically set the viewport start (scrollbar drag / wheel).
    /// Clamped to `[0, row_count - viewport]`. Sets `scroll_locked = true`
    /// so the discover-style anchor does not fight the manual scroll until
    /// the next cursor move.
    SetVScroll { position: usize },
    /// Set cursor to a specific filtered row index (mouse click, etc.).
    /// Clamped to visible range; clears `scroll_locked` (user is navigating).
    SetCursor { row: usize },
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsMsg {
    Message(ResultsMessage),
}

impl From<ResultsMessage> for ResultsMsg {
    fn from(m: ResultsMessage) -> Self {
        ResultsMsg::Message(m)
    }
}
