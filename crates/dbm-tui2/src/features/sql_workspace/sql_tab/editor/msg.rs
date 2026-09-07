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
    /// Clear the whole buffer and return to Insert mode after a successful
    /// editor-run query, mirroring the original dbm's `after_sql_run` (which
    /// clears the editor and restores Insert so the next statement can be typed
    /// immediately). Routed by the parent `sql_tab` when the run succeeds.
    ClearAfterRun,
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

    // —— Manual viewport scroll (drag / wheel) ——
    /// Scroll the editor viewport vertically by `delta` display rows.
    /// Positive = down, negative = up. Used by the scroll wheel.
    ScrollV { delta: i32 },
    /// Set the editor vertical viewport offset to `position` (display row).
    /// Used by scrollbar drag.
    SetVScroll { position: usize },
    /// Scroll the editor viewport horizontally by `delta` columns.
    /// Positive = right, negative = left. Note that edtui `wrap(true)` keeps
    /// `viewport_offset.x` at 0 during cursor-following render, so this only
    /// has effect when the editor is configured without wrapping — but we
    /// provide the message path anyway for consistency.
    ScrollH { delta: i32 },
    /// Set the editor horizontal viewport offset to `position` (column).
    SetHScroll { position: usize },

    // —— Mouse text selection ——
    /// A mouse Down/Drag/Up decoded by the pointer layer. The outcome is
    /// produced by edtui's own handler against a scratch copy (with the
    /// rendered hit area fed in), so `update` just applies the resulting
    /// cursor/mode/selection without touching terminal geometry.
    MouseGesture {
        outcome: super::mouse::MouseGestureOutcome,
    },
    /// Copy the current selection (if any) to the system clipboard. Raised by
    /// the copy shortcut (Cmd/Ctrl+C) while the editor is focused.
    CopySelection,
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
