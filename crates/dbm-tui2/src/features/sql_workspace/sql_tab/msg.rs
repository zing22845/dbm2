//! `sql_tab` feature messages.

use super::editor::msg::EditorMsg;
use super::history::msg::HistoryMsg;
use super::results::msg::ResultsMsg;

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
    ApplyContext { tab_id: usize, database: String, schema: String },
    /// Recall `sql` into the tab's editor (history apply).
    RecallHistory { tab_id: usize, sql: String },
    /// Run `sql` from the tab's editor (dispatched to results with session context).
    RunQueryFromEditor { tab_id: usize, sql: String },
    /// Open a new tab bound to a connection with its display identity.
    OpenConnectionTab {
        instance: String,
        connection: String,
        connection_id: String,
        database: Option<String>,
        schema: Option<String>,
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
