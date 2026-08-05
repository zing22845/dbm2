//! SQL completion sub-module messages.

use crate::common::utils::cursor::Cursor;

use super::provider::ColumnInfo;

/// The actual SQL completion messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlCompletionMessage {
    /// Recompute the completion items for the given SQL buffer at `cursor`.
    /// `tables`/`columns` are the metadata currently cached for the connection
    /// (empty when unavailable).
    Refresh {
        sql: String,
        cursor: Cursor,
        tables: Vec<String>,
        columns: Vec<ColumnInfo>,
    },
    /// Close the popup without applying.
    Close,
    /// Move the selection by `delta`.
    MoveSelection { delta: i32 },
    /// Apply the currently selected item (emits an `Apply` intent).
    Apply,
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlCompletionMsg {
    Message(SqlCompletionMessage),
}

impl From<SqlCompletionMessage> for SqlCompletionMsg {
    fn from(m: SqlCompletionMessage) -> Self {
        SqlCompletionMsg::Message(m)
    }
}
