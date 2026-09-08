//! Results list sub-module messages.

use crossterm::event::KeyEvent;

use super::super::edit_sql::EditTarget;
use super::super::pagination::ResultsPageAction;
use super::super::state::QueryResultData;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListMessage {
    /// Set the latest query result (replaces any previous result).
    SetResult {
        result: QueryResultData,
        paginated: bool,
    },
    /// The editability of the current result was resolved.
    EditabilityReady {
        target: Option<EditTarget>,
        blocked: Option<String>,
    },
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
    /// Copy the selected column's name (or all column names) to the clipboard.
    CopyColumnName,
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
    /// A `d` key press while editing: the original dbm's `dd` chord — the first
    /// press arms it, a second `d` within 500 ms deletes the selected row, a
    /// lone `d` does nothing.
    DelChord,
    /// An unmatched key pressed while an edit session is active cancels an
    /// armed `dd` chord (e.g. the `s` in a fast `d s d`). The key layer emits
    /// this so the chord only fires on two *consecutive* `d` presses.
    DelChordCancel,
    /// Apply a new rows-per-page limit.
    SetRowLimit { limit: usize },
    /// Jump to a page.
    SetPage { page: usize },
    /// Navigate by a page action (First / Prev / Next / Last).
    PageNav { action: ResultsPageAction },
    /// A `<` / `>` page key press: pages once and arms the double-press chord
    /// (`<<` / `>>` jump to first / last), mirroring the original dbm.
    PageChord { forward: bool },
    /// Count the total rows of the last query (COUNT over the query, requested
    /// from the `[c]count total rows` toolbar control / `c` key).
    CountRows,
    /// A COUNT total arrived (`sql` guards against applying a stale result
    /// after the query changed).
    CountReady { sql: String, total: Option<u64> },
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
