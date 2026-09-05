//! SQL workspace feature effects and actions.

use super::sql_tab::effect::{SqlTabAction, SqlTabEffect};
use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;

/// Actions produced by SQL workspace effects (from its child `sql_tab`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlAction {
    /// An action originating from the `sql_tab` child feature.
    SqlTab(SqlTabAction),
}

impl From<SqlTabAction> for SqlAction {
    fn from(a: SqlTabAction) -> Self {
        SqlAction::SqlTab(a)
    }
}

/// Effects emitted by the SQL workspace feature.
#[derive(Debug, Clone)]
pub enum SqlEffect {
    /// An effect originating from the `sql_tab` child feature.
    SqlTab(SqlTabEffect),
}

impl Effect for SqlEffect {
    type Action = SqlAction;

    fn run(
        self,
        emit: Emitter<Self::Action>,
        services: std::sync::Arc<Services>,
    ) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                SqlEffect::SqlTab(e) => {
                    let emit = emit.map::<SqlTabAction>();
                    e.run(emit, services)
                        .await
                        .into_iter()
                        .map(Into::into)
                        .collect()
                }
            }
        })
    }
}
