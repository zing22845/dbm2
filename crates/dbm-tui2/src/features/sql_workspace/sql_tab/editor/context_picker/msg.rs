//! Context picker sub-module messages.

use crossterm::event::KeyEvent;

use super::state::PickerColumn;

/// The actual context picker messages: open/close, column/cursor navigation,
/// `/` search input, apply, and the async catalog results from its effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextPickerMessage {
    /// Open the picker focused on `column`, seeded with the current connection
    /// context (`instance`/`connection`) and active `database`.
    Open { column: PickerColumn, instance: String, connection: String, database: String },
    /// Close the picker without applying.
    Close,
    /// Move the cursor in the active column by `delta` (`-1`/`+1`).
    MoveCursor { delta: i32 },
    /// Jump the cursor in `column` to a specific (filtered-list) index and
    /// focus that column. Emitted by a mouse click on a picker row.
    SetCursor { column: PickerColumn, cursor: usize },
    /// Switch the focused column.
    MoveColumn(PickerColumn),
    /// Begin `/` search input on the active column.
    BeginSearch,
    /// Forward a key event while a column's search input is active.
    SearchKey(KeyEvent),
    /// Commit the currently selected database/schema.
    Apply,
    /// The databases list for the connection was loaded.
    DatabasesLoaded { items: Vec<String> },
    /// Loading databases failed.
    DatabasesError { error: String },
    /// The schemas list for the previewed database was loaded.
    SchemasLoaded { items: Vec<String> },
    /// Loading schemas failed.
    SchemasError { error: String },
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextPickerMsg {
    Message(ContextPickerMessage),
}

impl From<ContextPickerMessage> for ContextPickerMsg {
    fn from(m: ContextPickerMessage) -> Self {
        ContextPickerMsg::Message(m)
    }
}
