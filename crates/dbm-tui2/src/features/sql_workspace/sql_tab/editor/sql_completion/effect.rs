//! SQL completion sub-module effects and actions.

use crate::app::action::Action;
use crate::app_shell::effect::Effect;

#[derive(Debug, Clone)]
pub enum SqlCompletionAction {}

impl From<SqlCompletionAction> for Action {
    fn from(_a: SqlCompletionAction) -> Self {
        match _a {}
    }
}

#[derive(Debug, Clone)]
pub enum SqlCompletionEffect {}

impl Effect for SqlCompletionEffect {
    type Action = SqlCompletionAction;

    fn run(
        self,
        _emit: crate::app_shell::effect::effect_trait::Emitter<Self::Action>,
        _services: std::sync::Arc<crate::common::service::services::Services>,
    ) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move { match self {} })
    }
}
