//! Results feature effects and actions.
//!
//! The only side effect is committing an edit batch. The pure edit session
//! (dirty cells / deleted / new rows → DML) is migrated; the actual DB
//! transaction execution depends on a live connection pool, which is not yet
//! wired into `Services`. `Commit` therefore reports a deferred status so the
//! structure is correct and ready for the connection plumbing.

use crate::app::action::Action;
use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;
use super::detail::effect::DetailEffect;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsAction {
    /// The edit batch commit outcome.
    CommitResult { ok: bool, message: String },
}

impl From<ResultsAction> for Action {
    fn from(a: ResultsAction) -> Self {
        // The commit outcome routing is not yet wired into the loop; the action
        // is produced but dropped for now (a status update is a later step).
        match a {
            ResultsAction::CommitResult { .. } => unreachable!(
                "ResultsAction::CommitResult routing not yet wired"
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ResultsEffect {
    /// Commit the edit batch. Executing requires a live connection pool; until
    /// the connection plumbing is wired this reports a deferred status.
    Commit { statements: Vec<String> },
    Detail(DetailEffect),
}

impl Effect for ResultsEffect {
    type Action = ResultsAction;

    fn run(self, _emit: Emitter<Self::Action>, _services: std::sync::Arc<Services>) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                ResultsEffect::Commit { statements } => {
                    // Deferred: no connection pool is wired yet. Replace with a
                    // `run_in_transaction` call once `Services` exposes a pool.
                    vec![ResultsAction::CommitResult {
                        ok: false,
                        message: format!(
                            "Commit requires a live connection ({} statement(s) prepared, not yet wired)",
                            statements.len()
                        ),
                    }]
                }
                ResultsEffect::Detail(_) => Vec::new(),
            }
        })
    }
}
