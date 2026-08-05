//! `sql_tab` feature intents.

use crate::app_shell::intent::Intent;
use super::msg::{SqlTabMessage, SqlTabMsg};
use super::editor::intent::EditorIntent;
use super::editor::context_picker::intent::ContextPickerIntent;
use super::history::intent::HistoryIntent;
use super::results::intent::ResultsIntent;

/// Intents emitted by the `sql_tab` feature. Each carries the `tab_id` of the
/// tab it originated from, so the routed message is delivered back to the same
/// tab (which may no longer be active).
#[derive(Debug, Clone)]
pub enum SqlTabIntent {
    /// An editor intent from the tab with `tab_id`.
    Editor { tab_id: usize, intent: EditorIntent },
    /// A results intent from the tab with `tab_id`.
    Results { tab_id: usize, intent: ResultsIntent },
    /// A history intent from the tab with `tab_id`.
    History { tab_id: usize, intent: HistoryIntent },
}

impl Intent for SqlTabIntent {
    type Message = SqlTabMsg;

    fn into_message(self) -> Self::Message {
        match self {
            // The context picker's `ApplyContext` intent must land on the tab's
            // session (a sibling of the editor), so it is resolved here into a
            // dedicated `SqlTabMessage::ApplyContext` rather than being
            // re-dispatched into the editor.
            SqlTabIntent::Editor {
                tab_id,
                intent: EditorIntent::ContextPicker(ContextPickerIntent::ApplyContext { database, schema }),
            } => SqlTabMsg::Message(SqlTabMessage::ApplyContext { tab_id, database, schema }),
            SqlTabIntent::Editor { tab_id, intent } => SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    tab_id,
                    msg: intent.into_message(),
                },
            ),
            SqlTabIntent::Results { tab_id, intent } => SqlTabMsg::Message(
                SqlTabMessage::Results {
                    tab_id,
                    msg: intent.into_message(),
                },
            ),
            // History Recall is resolved by `sql_tab` (it owns the editor).
            SqlTabIntent::History {
                tab_id,
                intent: HistoryIntent::Recall { sql },
            } => SqlTabMsg::Message(SqlTabMessage::RecallHistory { tab_id, sql }),
        }
    }
}
