//! `sql_tab` feature effects and actions.

use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;
use super::editor::effect::{EditorAction, EditorEffect};
use super::history::effect::{HistoryAction, HistoryEffect};
use super::results::effect::{ResultsAction, ResultsEffect};

/// Actions produced by `sql_tab` effects. Each carries the `tab_id` of the tab
/// it originated from so the resulting action can be routed back to the same
/// tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlTabAction {
    /// An editor action from the tab with `tab_id`.
    Editor { tab_id: usize, action: EditorAction },
    /// A results action from the tab with `tab_id`.
    Results { tab_id: usize, action: ResultsAction },
    /// A history action from the tab with `tab_id`.
    History { tab_id: usize, action: HistoryAction },
}

// Streaming emission from a child editor effect carries no tab context (the
// emitter cannot know which tab originated it). The context picker does not
// stream mid-run — it returns all actions from `run` with the correct `tab_id`
// — so this default is only a compile-time requirement of `Emitter::map`.
impl From<EditorAction> for SqlTabAction {
    fn from(action: EditorAction) -> Self {
        SqlTabAction::Editor { tab_id: 0, action }
    }
}

impl From<ResultsAction> for SqlTabAction {
    fn from(action: ResultsAction) -> Self {
        SqlTabAction::Results { tab_id: 0, action }
    }
}

impl From<HistoryAction> for SqlTabAction {
    fn from(action: HistoryAction) -> Self {
        SqlTabAction::History { tab_id: 0, action }
    }
}

/// Effects emitted by the `sql_tab` feature, delegating to its child modules.
/// Each carries the `tab_id` of the tab it originated from.
#[derive(Debug, Clone)]
pub enum SqlTabEffect {
    /// An editor effect from the tab with `tab_id`.
    Editor { tab_id: usize, effect: EditorEffect },
    /// A results effect from the tab with `tab_id`.
    Results { tab_id: usize, effect: ResultsEffect },
    /// A history effect from the tab with `tab_id`.
    History { tab_id: usize, effect: HistoryEffect },
}

impl Effect for SqlTabEffect {
    type Action = SqlTabAction;

    fn run(self, emit: Emitter<Self::Action>, services: std::sync::Arc<Services>) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                SqlTabEffect::Editor { tab_id, effect } => {
                    let emit = emit.map::<EditorAction>();
                    effect
                        .run(emit, services)
                        .await
                        .into_iter()
                        .map(|a| SqlTabAction::Editor { tab_id, action: a })
                        .collect()
                }
                SqlTabEffect::Results { tab_id, effect } => {
                    let emit = emit.map::<ResultsAction>();
                    effect
                        .run(emit, services)
                        .await
                        .into_iter()
                        .map(|a| SqlTabAction::Results { tab_id, action: a })
                        .collect()
                }
                SqlTabEffect::History { tab_id, effect } => {
                    let emit = emit.map::<HistoryAction>();
                    effect
                        .run(emit, services)
                        .await
                        .into_iter()
                        .map(|a| SqlTabAction::History { tab_id, action: a })
                        .collect()
                }
            }
        })
    }
}
