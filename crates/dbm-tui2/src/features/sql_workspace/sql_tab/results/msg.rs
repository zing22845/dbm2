//! Results feature messages.

use crossterm::event::KeyEvent;

use super::detail::msg::DetailMsg;
use super::state::QueryResultData;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsMessage {
    /// Set the latest query result (replaces any previous result).
    SetResult { result: QueryResultData, paginated: bool },
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
    /// Commit the current edits (emits a commit effect / intent).
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
