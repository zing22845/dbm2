//! History feature effects and actions.

use crate::app::action::Action;
use crate::app_shell::effect::Effect;

#[derive(Debug, Clone)]
pub enum HistoryAction {}

impl From<HistoryAction> for Action {
    fn from(_a: HistoryAction) -> Self {
        match _a {}
    }
}

#[derive(Debug, Clone)]
pub enum HistoryEffect {}

impl Effect for HistoryEffect {
    type Action = HistoryAction;

    fn run(self, _emit: crate::app_shell::effect::effect_trait::Emitter<Self::Action>, _services: std::sync::Arc<crate::common::service::services::Services>) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {}
        })
    }
}
