//! Results feature messages.

use crossterm::event::KeyEvent;

use super::detail::msg::DetailMsg;
use super::edit_sql::EditTarget;
use super::state::QueryResultData;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsMessage {
    /// Set the latest query result (replaces any previous result).
    SetResult { result: QueryResultData, paginated: bool },
    /// The editability of the current result was resolved (edit target or the
    /// reason it cannot be edited).
    EditabilityReady { target: Option<EditTarget>, blocked: Option<String> },
    /// Clear the current result (e.g. after a failed query).
    ClearResult,
    /// Move the cell selection by `(dr, dc)`.
    MoveSelection { dr: i32, dc: i32 },
    /// Begin `/` search input.
    BeginSearch,
    /// Forward a key while search input is active.
    SearchKey(KeyEvent),
    /// Reset the result selection / scroll (after a new result).
    ResetSelection,
    /// Run a SQL query against the tab's connection (emits a RunQuery effect).
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
    /// Set the detail draft text (edited cell value).
    SetDetailDraft { text: String },
    /// Apply a new rows-per-page limit (from the row-limit picker modal).
    SetRowLimit { limit: usize },
    /// Jump to a page (from the page-input modal).
    SetPage { page: usize },
    /// Commit the current edits against the tab's connection (emits a
    /// transaction-executing `ResultsEffect::Commit`).
    Commit,
    /// Forward to the detail sub-module.
    Detail(DetailMsg),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsMsg {
    Message(ResultsMessage),
}

impl From<ResultsMessage> for ResultsMsg {
    fn from(m: ResultsMessage) -> Self {
        ResultsMsg::Message(m)
    }
}

impl From<DetailMsg> for ResultsMsg {
    fn from(m: DetailMsg) -> Self {
        ResultsMsg::Message(ResultsMessage::Detail(m))
    }
}
