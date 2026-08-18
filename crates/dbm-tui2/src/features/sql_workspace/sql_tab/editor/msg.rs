//! Editor feature messages.

use crossterm::event::KeyEvent;
use std::collections::HashMap;

use super::context_picker::msg::ContextPickerMsg;
use super::sql_completion::msg::SqlCompletionMsg;
use super::sql_completion::provider::ColumnInfo;

/// The actual editor messages: buffer edits plus forwarding to sub-modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorMessage {
    /// Forward a normalized key to the editor buffer.
    KeyEvent {
        key: KeyEvent,
        tracked_caps_lock: bool,
    },
    /// Paste `text` at the cursor.
    Paste { text: String },
    /// Replace the whole buffer with `sql`.
    SetSql { sql: String },
    /// Run the current editor SQL (emits a RunQuery intent resolved by sql_tab).
    Run,
    /// Force the SQL-completion popup open at the current buffer/cursor,
    /// bypassing the auto-open gate (Shift+Tab; mirrors the original dbm's
    /// `completion_trigger_key`).
    ForceCompletion,
    /// Recompute the SQL-completion popup for the current buffer/cursor using
    /// the normal auto-open gate. Used to re-evaluate the popup after a setting
    /// that affects completion (e.g. toggling TblCmp) changes.
    RefreshCompletion,
    /// The SQL-completion catalog for the tab's connection/schema was loaded.
    CatalogLoaded {
        tables: Vec<String>,
        columns_by_table: HashMap<String, Vec<ColumnInfo>>,
    },
    /// Forward to the context picker sub-module.
    ContextPicker(ContextPickerMsg),
    /// Forward to the SQL completion sub-module.
    SqlCompletion(SqlCompletionMsg),
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorMsg {
    Message(EditorMessage),
}

impl From<EditorMessage> for EditorMsg {
    fn from(m: EditorMessage) -> Self {
        EditorMsg::Message(m)
    }
}

impl From<ContextPickerMsg> for EditorMsg {
    fn from(m: ContextPickerMsg) -> Self {
        EditorMsg::Message(EditorMessage::ContextPicker(m))
    }
}
impl From<SqlCompletionMsg> for EditorMsg {
    fn from(m: SqlCompletionMsg) -> Self {
        EditorMsg::Message(EditorMessage::SqlCompletion(m))
    }
}
