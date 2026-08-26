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
    /// Begin `/` search input.
    BeginSearch,
    /// Forward a key while search input is active.
    SearchKey(KeyEvent),
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
    SyncViewport { rows: usize },
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