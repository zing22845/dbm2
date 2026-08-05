//! `sql_tab` feature effects and actions.

use crate::app::action::Action;
use crate::app_shell::effect::Effect;
use super::editor::effect::EditorEffect;
use super::history::effect::HistoryEffect;
use super::results::effect::ResultsEffect;

/// Actions produced by `sql_tab` effects.
#[derive(Debug, Clone)]
pub enum SqlTabAction {}

impl From<SqlTabAction> for Action {
    fn from(_a: SqlTabAction) -> Self {
        match _a {}
    }
}

/// Effects emitted by the `sql_tab` feature. Each carries the `tab_id` of the
/// tab it originated from so the resulting action can be routed back to the
/// same tab.
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

    fn run(self, _emit: crate::app_shell::effect::effect_trait::Emitter<Self::Action>, _services: std::sync::Arc<crate::common::service::services::Services>) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                SqlTabEffect::Editor { .. }
                | SqlTabEffect::Results { .. }
                | SqlTabEffect::History { .. } => Vec::new(),
            }
        })
    }
}
