//! `sql_tab` feature intents.

use crate::app_shell::intent::Intent;
use super::msg::{SqlTabMessage, SqlTabMsg};
use super::editor::intent::EditorIntent;
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

    // Skeleton state: the child feature messages (`EditorMsg`, `ResultsMsg`,
    // `HistoryMsg`) are currently uninhabited because their leaf messages are
    // empty enums. The expressions below are thus unreachable until real
    // business messages are introduced; remove this allow then.
    #[allow(unreachable_code)]
    fn into_message(self) -> Self::Message {
        match self {
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
            SqlTabIntent::History { tab_id, intent } => SqlTabMsg::Message(
                SqlTabMessage::History {
                    tab_id,
                    msg: intent.into_message(),
                },
            ),
        }
    }
}
