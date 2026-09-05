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
use super::state::QueryResultData;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsMessage {
    // ---- Direct variants (for external callers) ----
    /// Set the latest query result.
    SetResult { result: QueryResultData, paginated: bool },
    /// The editability of the current result was resolved.
    EditabilityReady { target: Option<EditTarget>, blocked: Option<String> },
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
    /// Set the detail draft text (edited cell value).
    SetDetailDraft { text: String },
    /// Apply a new rows-per-page limit.
    SetRowLimit { limit: usize },
    /// Jump to a page.
    SetPage { page: usize },
    /// Commit the current edits.
    Commit,
    /// Toggle the detail sub-pane open/close (inspect mode).
    ToggleDetail,
    /// Synchronise the viewport dimensions.
    SyncViewport { rows: usize, width: u16 },
    /// Set vertical scroll offset (from scrollbar drag).
    SetVScroll { position: usize },
    /// Set horizontal scroll offset (from scrollbar drag).
    SetHScroll { position: usize },
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
            ResultsMessage::SetRowLimit { limit } => ListMessage::SetRowLimit { limit },
            ResultsMessage::SetPage { page } => ListMessage::SetPage { page },
            ResultsMessage::Commit => ListMessage::Commit,
            ResultsMessage::SyncViewport { rows, width } => ListMessage::SyncViewport { rows, width },
            ResultsMessage::SetVScroll { position } => ListMessage::SetVScroll { position },
            ResultsMessage::SetHScroll { position } => ListMessage::SetHScroll { position },
            ResultsMessage::AdjustColWidth { delta } => ListMessage::AdjustColWidth { delta },
            ResultsMessage::AdjustColWidthTo { col, width } => {
                ListMessage::AdjustColWidthTo { col, width }
            }
            ResultsMessage::SetDetailDraft { .. } | ResultsMessage::ToggleDetail => {
                ListMessage::ResetSelection
            }
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