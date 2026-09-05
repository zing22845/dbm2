//! `sql_tab` feature intents.

use super::editor::context_picker::intent::ContextPickerIntent;
use super::editor::intent::EditorIntent;
use super::history::intent::HistoryIntent;
use super::msg::{SqlTabMessage, SqlTabMsg};
use super::results::intent::ResultsIntent;
use crate::app_shell::intent::Intent;

/// Intents emitted by the `sql_tab` feature. Each carries the `tab_id` of the
/// tab it originated from, so the routed message is delivered back to the same
/// tab (which may no longer be active).
#[derive(Debug, Clone)]
pub enum SqlTabIntent {
    /// An editor intent from the tab with `tab_id`.
    Editor { tab_id: usize, intent: EditorIntent },
    /// A results intent from the tab with `tab_id`.
    Results {
        tab_id: usize,
        intent: ResultsIntent,
    },
    /// A history intent from the tab with `tab_id`.
    History {
        tab_id: usize,
        intent: HistoryIntent,
    },
}

impl Intent for SqlTabIntent {
    type Message = SqlTabMsg;

    fn into_message(self) -> Option<Self::Message> {
        match self {
            // The context picker's `ApplyContext` intent must land on the tab's
            // session (a sibling of the editor), so it is resolved here into a
            // dedicated `SqlTabMessage::ApplyContext` rather than being
            // re-dispatched into the editor.
            SqlTabIntent::Editor {
                tab_id,
                intent:
                    EditorIntent::ContextPicker(ContextPickerIntent::ApplyContext { database, schema }),
            } => Some(SqlTabMsg::Message(SqlTabMessage::ApplyContext {
                tab_id,
                database,
                schema,
            })),
            // The editor's Run intent is resolved by sql_tab (it owns the
            // connection context), so it becomes a dedicated message.
            SqlTabIntent::Editor {
                tab_id,
                intent: EditorIntent::RunQuery { sql },
            } => Some(SqlTabMsg::Message(SqlTabMessage::RunQueryFromEditor {
                tab_id,
                sql,
            })),
            // The editor's catalog-load request is resolved by sql_tab (which
            // owns the connection context) into a dedicated reload message.
            SqlTabIntent::Editor {
                tab_id,
                intent: EditorIntent::LoadCompletionCatalog,
            } => Some(SqlTabMsg::Message(SqlTabMessage::ReloadCompletionCatalog {
                tab_id,
            })),
            // The editor's `ctrl+r` history recall is resolved by sql_tab into a
            // dedicated message (pins the entry and moves focus to History).
            SqlTabIntent::Editor {
                tab_id,
                intent: EditorIntent::HistoryRecall,
            } => Some(SqlTabMsg::Message(SqlTabMessage::EnterHistoryRecall {
                tab_id,
            })),
            SqlTabIntent::Editor { tab_id, intent } => {
                // The child editor intent may itself be cross-feature (a
                // `RunQuery`/`ApplyContext` that was not intercepted above),
                // in which case it declines a message and we skip routing.
                intent
                    .into_message()
                    .map(|msg| SqlTabMsg::Message(SqlTabMessage::Editor { tab_id, msg }))
            }
            SqlTabIntent::Results { tab_id, intent } => intent
                .into_message()
                .map(|msg| SqlTabMsg::Message(SqlTabMessage::Results { tab_id, msg })),
            // History Recall is resolved by `sql_tab` (it owns the editor).
            SqlTabIntent::History {
                tab_id,
                intent: HistoryIntent::Recall { sql },
            } => Some(SqlTabMsg::Message(SqlTabMessage::RecallHistory {
                tab_id,
                sql,
            })),
            // Any other history intent (e.g. `RecordSuccess`) is a plain
            // sub-module message, routed back to the same tab.
            SqlTabIntent::History { tab_id, intent } => intent
                .into_message()
                .map(|msg| SqlTabMsg::Message(SqlTabMessage::History { tab_id, msg })),
        }
    }
}
