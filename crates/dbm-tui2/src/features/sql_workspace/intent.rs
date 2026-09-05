//! SQL workspace feature intents.

use super::msg::SqlMsg;
use super::sql_tab::intent::SqlTabIntent;
use crate::app_shell::intent::Intent;

/// Intents emitted by the SQL workspace feature. Child intents are wrapped
/// so they can be lifted into the global router via `SqlMsg`.
#[derive(Debug, Clone)]
pub enum SqlIntent {
    /// An intent originating from the `sql_tab` child feature.
    SqlTab(SqlTabIntent),
}

impl Intent for SqlIntent {
    type Message = SqlMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {
            SqlIntent::SqlTab(i) => i.into_message().map(Into::into),
        }
    }
}
