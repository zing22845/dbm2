//! SQL completion sub-module messages.

use crate::common::utils::cursor::Cursor;

use super::provider::ColumnInfo;

/// The actual SQL completion messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlCompletionMessage {
    /// Recompute the completion items for the given SQL buffer at `cursor`.
    /// `tables`/`columns` are the metadata currently cached for the connection
    /// (empty when unavailable). `explicit` is true when the user asked for
    /// completion directly (Shift+Tab) — that forces the popup open, bypassing
    /// the auto-open gate (matching the original dbm's `completion_trigger_key`).
    /// `complete_table_names` is the TblCmp flag: when OFF, a table-intent slot
    /// offers keyword completion instead of table names (original dbm's
    /// `table_completion_allowed`).
    Refresh {
        sql: String,
        cursor: Cursor,
        tables: Vec<String>,
        columns: Vec<ColumnInfo>,
        explicit: bool,
        complete_table_names: bool,
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
