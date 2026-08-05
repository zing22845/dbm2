//! SQL workspace feature effects and actions.

use crate::app_shell::effect::Effect;
use super::sql_tab::effect::SqlTabEffect;

/// Actions produced by SQL workspace effects.
#[derive(Debug, Clone)]
pub enum SqlAction {}

/// Effects emitted by the SQL workspace feature.
#[derive(Debug, Clone)]
pub enum SqlEffect {
    /// An effect originating from the `sql_tab` child feature.
    SqlTab(SqlTabEffect),
}

impl Effect for SqlEffect {
    type Action = SqlAction;

    fn run(self, _emit: crate::app_shell::effect::effect_trait::Emitter<Self::Action>, _services: std::sync::Arc<crate::common::service::services::Services>) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
            // Child effects resolve to child actions, which are lifted into
            // the global action stream. Skeleton: no effects yet.
            SqlEffect::SqlTab(_e) => Vec::new(),
        }
        })
    }
}
