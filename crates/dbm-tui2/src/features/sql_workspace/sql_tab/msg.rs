//! `sql_tab` feature messages.

use super::editor::msg::EditorMsg;
use super::history::msg::HistoryMsg;
use super::results::msg::ResultsMsg;
use super::state::SqlFocus;

/// The actual `sql_tab` messages: tab management plus forwarding to the
/// three child modules. Child messages carry a `tab_id` so they target a
/// specific tab (which may not be the active one, e.g. an async result).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlTabMessage {
    /// Switch the active tab by index.
    Tab(usize),
    /// Open a new tab.
    OpenTab,
    /// Close the tab at `idx`.
    CloseTab(usize),
    /// Apply a chosen database/schema context to the tab's session.
    ApplyContext {
        tab_id: usize,
        database: String,
        schema: String,
    },
    /// Recall `sql` into the tab's editor (history apply).
    RecallHistory { tab_id: usize, sql: String },
    /// Enter history recall from the editor (`ctrl+r`): pin the most recent
    /// entry and move focus to the History pane (mirrors the original dbm's
    /// `enter_history_recall_from_sql`).
    EnterHistoryRecall { tab_id: usize },
    /// Set the active tab's sub-pane focus (editor / results / history).
    Focus(SqlFocus),
    /// Run `sql` from the tab's editor (dispatched to results with session context).
    RunQueryFromEditor { tab_id: usize, sql: String },
    /// Open (or focus) the connection's active tab, fill it with a
    /// `SELECT * FROM "schema"."table"` query for `table`, and run it
    /// immediately. Mirrors the original dbm's double-click-on-table behavior:
    /// it runs a data query in the active tab and moves focus to Results.
    RunTableQuery {
        instance: String,
        connection: String,
        connection_id: String,
        database: Option<String>,
        schema: Option<String>,
        table: String,
        /// The schema that qualifies `table` in the generated SQL. Falls back
        /// to `schema` when `None`.
        table_schema: Option<String>,
    },
    /// Toggle table-name completion (TblCmp) for the tab, in INSERT mode
    /// (Alt+Tab; Ctrl+T is reserved for theme toggling in dbm2).
    ToggleTableCompletion { tab_id: usize },
    /// Reload the tab's SQL-completion catalog. Raised by the editor when it
    /// hits a table-intent slot (TblCmp ON) without cached table names;
    /// resolved into a `LoadCompletionCatalog` effect here.
    ReloadCompletionCatalog { tab_id: usize },
    /// Open a new tab bound to a connection with its display identity.
    OpenConnectionTab {
        instance: String,
        connection: String,
        connection_id: String,
        database: Option<String>,
        schema: Option<String>,
        /// The connection's configured default database, used as the fallback
        /// context when the connection has no existing tab (case A).
        default_database: Option<String>,
    },
    /// Focus an existing tab for the connection if one exists, else open a new
    /// one (the connection-row Enter behavior, mirroring original dbm).
    FocusConnectionTab {
        instance: String,
        connection: String,
        connection_id: String,
        database: Option<String>,
        schema: Option<String>,
        /// The connection's configured default database, used as the fallback
        /// context when the connection has no existing tab (case A).
        default_database: Option<String>,
    },
    /// Switch to a connection's tabs without opening a new tab. When the
    /// explorer tree selection changes to a different connection, the shell
    /// sends this so the tab bar only shows that connection's tabs.
    SetActiveConnection {
        instance: String,
        connection: String,
    },
    /// Set the editor top-pane height in rows (horizontal splitter), e.g. from
    /// a mouse drag on the editor/results splitter.
    SetEditorTopHeight { tab_id: usize, height: u16 },
    /// Nudge the editor top-pane height by one step with `+` / `-` (`plus` is
    /// true for `+`), growing the currently-focused pane. The tab resolves the
    /// top/bottom focus.
    NudgeEditorTopHeight { tab_id: usize, plus: bool },
    /// Set the history pane width in columns (vertical splitter), e.g. from a
    /// mouse drag on the editor/history splitter.
    SetHistoryWidth { tab_id: usize, width: u16 },
    /// Set the History detail pane width (dragging the internal detail/list
    /// splitter). The total History zone width stays constant; only the
    /// detail-vs-list split re-allocates.
    SetHistoryDetailWidth { tab_id: usize, width: u16 },
    /// Set the Results detail pane width (dragging the Results-internal
    /// detail/list splitter).
    SetResultsDetailWidth { tab_id: usize, width: u16 },
    /// Replace the tab's in-memory SQL history store with a freshly loaded
    /// snapshot (issued when persisted history is loaded for a tab).
    SetHistoryStore {
        tab_id: usize,
        store: crate::features::sql_workspace::sql_tab::history::store::SqlHistoryStore,
    },
    /// Nudge the History detail pane width by one keyboard step (`[` shrinks,
    /// `]` grows — the detail is the LEFT side of the internal splitter).
    NudgeHistoryDetailWidth {
        tab_id: usize,
        nudge: crate::common::layout::splitter::VerticalSplitterNudge,
    },
    /// Nudge the Results detail pane width (same `[` shrinks, `]` grows
    /// convention — detail is on the RIGHT side so `]` shrinks the detail).
    NudgeResultsDetailWidth {
        tab_id: usize,
        nudge: crate::common::layout::splitter::VerticalSplitterNudge,
    },
    /// Nudge the history pane width by one keyboard step (`[` grows, `]`
    /// shrinks — history owns the right side of the editor/history splitter).
    NudgeHistoryWidth {
        tab_id: usize,
        nudge: crate::common::layout::splitter::VerticalSplitterNudge,
    },
    /// Forwarded editor message, targeted at the tab with `tab_id`.
    Editor { tab_id: usize, msg: EditorMsg },
    /// Forwarded results message, targeted at the tab with `tab_id`.
    Results { tab_id: usize, msg: ResultsMsg },
    /// Forwarded history message, targeted at the tab with `tab_id`.
    History { tab_id: usize, msg: HistoryMsg },
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlTabMsg {
    Message(SqlTabMessage),
}

impl From<SqlTabMessage> for SqlTabMsg {
    fn from(m: SqlTabMessage) -> Self {
        SqlTabMsg::Message(m)
    }
}

// Note: there is intentionally no `From<EditorMsg>` (etc.) conversion here.
// A child message must be paired with a `tab_id` to be routed, so callers
// construct `SqlTabMessage::Editor { tab_id, msg }` explicitly.
