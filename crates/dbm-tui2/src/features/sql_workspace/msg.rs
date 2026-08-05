//! SQL workspace feature messages.

use super::sql_tab::msg::SqlTabMsg;

/// The actual SQL-workspace messages. In the skeleton the only message is a
/// forwarded `SqlTab` message; business logic will add workspace-level
/// variants (e.g. tab open/close/switch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlMessage {
    /// Messages routed to the `sql_tab` child feature.
    SqlTab(SqlTabMsg),
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlMsg {
    /// A real SQL-workspace message.
    Message(SqlMessage),
}

impl From<SqlMessage> for SqlMsg {
    fn from(m: SqlMessage) -> Self {
        SqlMsg::Message(m)
    }
}

// Lift child messages directly into the workspace envelope.
impl From<SqlTabMsg> for SqlMsg {
    fn from(m: SqlTabMsg) -> Self {
        SqlMsg::Message(SqlMessage::SqlTab(m))
    }
}
