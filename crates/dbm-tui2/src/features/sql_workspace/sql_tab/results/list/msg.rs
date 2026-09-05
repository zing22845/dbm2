//! Results list sub-module messages.

use crossterm::event::KeyEvent;

use super::super::edit_sql::EditTarget;
use super::super::state::QueryResultData;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListMessage {
    /// Set the latest query result (replaces any previous result).
    SetResult { result: QueryResultData, paginated: bool },
    /// The editability of the current result was resolved.
    EditabilityReady { target: Option<EditTarget>, blocked: Option<String> },
    /// Clear the current result.
    ClearResult,
    /// A query failed: clear the current result and store the error message.
    QueryError { message: String },
    /// Move the cell selection by `(dr, dc)`.
    MoveSelection { dr: i32, dc: i32 },
    /// Set the cell selection to an absolute `(row, col)` (from a mouse click).
    SetSelection { row: usize, col: usize },
    /// Begin `/` search input.
    BeginSearch,
    /// Forward a key while search input is active.
    SearchKey(KeyEvent),
    /// Move to the next/previous match while an applied filter is shown
    /// (`n` / `N` outside of input mode), wrapping.
    SearchNavigate { forward: bool },
    /// Reset the result selection / scroll (after a new result).
    ResetSelection,
    /// Enter / toggle edit mode.
    EnterEdit,
    /// Exit edit mode (clears the session).
    ExitEdit,
    /// Roll back all edits.
    Rollback,
    /// Add a pending insert row.
    AddRow,
    /// Duplicate the selected row as a pending insert.
    DupRow,
    /// Delete the selected row.
    DelRow,
    /// Apply a new rows-per-page limit.
    SetRowLimit { limit: usize },
    /// Jump to a page.
    SetPage { page: usize },
    /// Run a SQL query.
    RunQuery {
        instance: String,
        connection: String,
        database: Option<String>,
        schema: String,
        sql: String,
        paginated: bool,
        page: usize,
        row_limit: usize,
    },
    /// Commit the current edits.
    Commit,
    /// Synchronise the viewport dimensions (called after render).
    SyncViewport { rows: usize, width: u16 },
    /// Set vertical scroll offset (from scrollbar drag).
    SetVScroll { position: usize },
    /// Set horizontal scroll offset (from scrollbar drag).
    SetHScroll { position: usize },
    /// Scroll horizontally by `delta` columns (shift+wheel), relative to the
    /// current offset. Clamped to the same bound as `SetHScroll`.
    ScrollHScroll { delta: i32 },
    /// Adjust the selected column's width by `delta` columns (clamped) —
    /// the `,` / `.` column-width shortcuts.
    AdjustColWidth { delta: i16 },
    /// Set column `col`'s width to `width` columns (clamped) — from a mouse
    /// drag on that column's header splitter.
    AdjustColWidthTo { col: usize, width: u16 },
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