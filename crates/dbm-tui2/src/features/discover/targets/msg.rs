//! Discovery targets editor feature messages.

use super::state::TargetCol;

/// The actual targets editor messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetsMessage {
    /// Move the row cursor up.
    MoveUp,
    /// Move the row cursor down.
    MoveDown,
    /// Move the column cursor to the host column.
    MoveColHost,
    /// Move the column cursor to the ports column.
    MoveColPorts,
    /// Insert a new empty row after the cursor.
    AddRow,
    /// Delete the row under the cursor.
    DeleteRow,
    /// Begin an inline edit of the focused cell.
    BeginEdit,
    /// Commit the in-progress inline edit.
    CommitEdit,
    /// Discard the in-progress inline edit.
    CancelEdit,
    /// Insert a character into the edit buffer.
    EditChar(char),
    /// Backspace in the edit buffer.
    EditBackspace,
    /// Move the edit cursor left.
    EditCursorLeft,
    /// Move the edit cursor right.
    EditCursorRight,
    /// Undo the last target-list mutation.
    Undo,
    /// Redo the last undone target-list mutation.
    Redo,
    /// Paste TSV target rows (or text into the edit buffer).
    Paste(String),
    /// Programmatically commit a cell value (used by the shell for validation
    /// feedback).
    CommitCell { row: usize, col: TargetCol, value: String },
    /// Select a specific row (mouse click on a row).
    SelectRow { row: usize },
    /// Select a specific cell (mouse click on a cell).
    SelectCell { row: usize, col: TargetCol },
    /// Begin edit on a specific cell (double-click on a cell).
    BeginEditCell { row: usize, col: TargetCol },
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetsMsg {
    Message(TargetsMessage),
}

impl From<TargetsMessage> for TargetsMsg {
    fn from(m: TargetsMessage) -> Self {
        TargetsMsg::Message(m)
    }
}
