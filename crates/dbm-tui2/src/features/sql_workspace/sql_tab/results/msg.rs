//! Results feature messages.
//!
//! Supports both direct variants (for external callers like `input.rs` and
//! `sql_tab/update.rs`) and routed variants (`List(ListMsg)`, `Detail(DetailMsg)`)
//! for internal sub-feature routing.

use crossterm::event::KeyEvent;

use super::detail::msg::DetailMsg;
use super::edit_sql::EditTarget;
use super::list::msg::ListMessage;
pub use super::list::msg::ListMsg;
use super::pagination::ResultsPageAction;
use super::state::QueryResultData;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsMessage {
    // ---- Direct variants (for external callers) ----
    /// Set the latest query result.
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
    /// A query failed.
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
    /// Copy the selected column's name (or all names) to the clipboard.
    CopyColumnName,
    /// Reset the result selection / scroll.
    ResetSelection,
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
    /// Enter / toggle edit mode.
    EnterEdit,
    /// Exit edit mode.
    ExitEdit,
    /// Roll back all edits.
    Rollback,
    /// Add a pending insert row.
    AddRow,
    /// Duplicate the selected row.
    DupRow,
    /// Delete the selected row.
    DelRow,
    /// A `d` key press while editing (the original `dd` chord — see
    /// [`ListMessage::DelChord`]).
    DelChord,
    /// An Esc / focus-leave attempt was made while the edit session has
    /// unsaved changes: block the leave and surface the reason on the results
    /// footer instead of silently dropping the edits.
    EditLeaveAttempt,
    /// Set the detail draft text (edited cell value).
    SetDetailDraft { text: String },
    /// Focus the detail cell editor on the current selection (opens the detail
    /// pane if needed). Requires an active whole-result edit session; the
    /// current cell value is loaded as the draft baseline.
    FocusDetail,
    /// Leave the detail cell editor back to the table view. Blocked while the
    /// draft is dirty (the footer shows the save/discard hint instead).
    UnfocusDetail,
    /// Save the detail draft to the selected cell (`dirty_cells` + live row),
    /// then treat the draft as clean. The detail editor stays focused.
    SaveDetailCell,
    /// Discard the current detail draft back to the cell's baseline.
    DiscardDetailCell,
    /// Forward a key into the focused detail cell editor.
    DetailEditorKey(KeyEvent),
    /// Copy the focused detail cell editor's selection to the clipboard (the
    /// platform copy chord, mirroring the original dbm's `copy_active_selection`
    /// which probes the SQL editor first, then the results detail editor).
    CopyDetailSelection,
    /// Apply a new rows-per-page limit.
    SetRowLimit { limit: usize },
    /// Jump to a page.
    SetPage { page: usize },
    /// Navigate by a page action (First / Prev / Next / Last).
    PageNav { action: ResultsPageAction },
    /// A `<` / `>` page key press (see [`ListMessage::PageChord`]).
    PageChord { forward: bool },
    /// Count the total rows of the last query (see [`ListMessage::CountRows`]).
    CountRows,
    /// A COUNT total arrived (see [`ListMessage::CountReady`]).
    CountReady { sql: String, total: Option<u64> },
    /// Commit the current edits.
    Commit,
    /// A commit finished (`ok`). The round surfaces its status in the global
    /// footer; on success this message drives the post-commit flow (exit edit
    /// mode and re-run the query so the committed rows refresh).
    CommitOutcome { ok: bool },
    /// Toggle the detail sub-pane open/close (inspect mode).
    ToggleDetail,
    /// Synchronise the viewport dimensions.
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

    // ---- Routed variants (for internal sub-feature routing) ----
    /// Route to the list sub-feature.
    List(ListMsg),
    /// Route to the detail sub-feature.
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

impl From<ListMsg> for ResultsMsg {
    fn from(m: ListMsg) -> Self {
        ResultsMsg::Message(ResultsMessage::List(m))
    }
}

impl From<DetailMsg> for ResultsMsg {
    fn from(m: DetailMsg) -> Self {
        ResultsMsg::Message(ResultsMessage::Detail(m))
    }
}

impl ResultsMessage {
    /// Convert a direct variant into its `List(ListMessage)` equivalent.
    pub fn into_list_message(self) -> ListMessage {
        match self {
            ResultsMessage::SetResult { result, paginated } => {
                ListMessage::SetResult { result, paginated }
            }
            ResultsMessage::EditabilityReady { target, blocked } => {
                ListMessage::EditabilityReady { target, blocked }
            }
            ResultsMessage::ClearResult => ListMessage::ClearResult,
            ResultsMessage::QueryError { message } => ListMessage::QueryError { message },
            ResultsMessage::MoveSelection { dr, dc } => ListMessage::MoveSelection { dr, dc },
            ResultsMessage::SetSelection { row, col } => ListMessage::SetSelection { row, col },
            ResultsMessage::BeginSearch => ListMessage::BeginSearch,
            ResultsMessage::SearchKey(key) => ListMessage::SearchKey(key),
            ResultsMessage::SearchNavigate { forward } => ListMessage::SearchNavigate { forward },
            ResultsMessage::CopyColumnName => ListMessage::CopyColumnName,
            ResultsMessage::ResetSelection => ListMessage::ResetSelection,
            ResultsMessage::RunQuery {
                instance,
                connection,
                database,
                schema,
                sql,
                paginated,
                page,
                row_limit,
            } => ListMessage::RunQuery {
                instance,
                connection,
                database,
                schema,
                sql,
                paginated,
                page,
                row_limit,
            },
            ResultsMessage::EnterEdit => ListMessage::EnterEdit,
            ResultsMessage::ExitEdit => ListMessage::ExitEdit,
            ResultsMessage::Rollback => ListMessage::Rollback,
            ResultsMessage::AddRow => ListMessage::AddRow,
            ResultsMessage::DupRow => ListMessage::DupRow,
            ResultsMessage::DelRow => ListMessage::DelRow,
            ResultsMessage::DelChord => ListMessage::DelChord,
            ResultsMessage::SetRowLimit { limit } => ListMessage::SetRowLimit { limit },
            ResultsMessage::SetPage { page } => ListMessage::SetPage { page },
            ResultsMessage::PageNav { action } => ListMessage::PageNav { action },
            ResultsMessage::PageChord { forward } => ListMessage::PageChord { forward },
            ResultsMessage::CountRows => ListMessage::CountRows,
            ResultsMessage::CountReady { sql, total } => ListMessage::CountReady { sql, total },
            ResultsMessage::Commit => ListMessage::Commit,
            ResultsMessage::SyncViewport { rows, width } => {
                ListMessage::SyncViewport { rows, width }
            }
            ResultsMessage::SetVScroll { position } => ListMessage::SetVScroll { position },
            ResultsMessage::SetHScroll { position } => ListMessage::SetHScroll { position },
            ResultsMessage::ScrollHScroll { delta } => ListMessage::ScrollHScroll { delta },
            ResultsMessage::AdjustColWidth { delta } => ListMessage::AdjustColWidth { delta },
            ResultsMessage::AdjustColWidthTo { col, width } => {
                ListMessage::AdjustColWidthTo { col, width }
            }
            ResultsMessage::SetDetailDraft { .. }
            | ResultsMessage::FocusDetail
            | ResultsMessage::UnfocusDetail
            | ResultsMessage::SaveDetailCell
            | ResultsMessage::DiscardDetailCell
            | ResultsMessage::DetailEditorKey(_)
            | ResultsMessage::CopyDetailSelection
            | ResultsMessage::ToggleDetail
            | ResultsMessage::CommitOutcome { .. }
            | ResultsMessage::EditLeaveAttempt => ListMessage::ResetSelection,
            ResultsMessage::List(_) | ResultsMessage::Detail(_) => {
                panic!("into_list_message called on already-routed message")
            }
        }
    }

    /// Whether this is a direct variant that should be routed to the list
    /// sub-feature (most direct variants).
    pub fn is_list_direct(&self) -> bool {
        !matches!(
            self,
            ResultsMessage::Detail(_) | ResultsMessage::SetDetailDraft { .. }
        )
    }

    /// Whether this is a direct variant that should be routed to the detail
    /// sub-feature.
    pub fn is_detail_direct(&self) -> bool {
        matches!(self, ResultsMessage::SetDetailDraft { .. })
    }
}
