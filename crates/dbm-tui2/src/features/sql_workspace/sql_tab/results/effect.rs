//! Results feature effects and actions.
//!
//! The side effects are running a SQL query (via `Services::execute_sql`) and
//! committing an edit batch. The commit's DB transaction execution still
//! depends on a live pool being wired into `Services`; until then it reports a
//! deferred status. Query execution is fully wired.

use crate::app::action::Action;
use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;
use super::detail::effect::DetailEffect;
use super::state::QueryResultData;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsAction {
    /// A query completed with a result.
    ResultReady { result: QueryResultData, paginated: bool },
    /// A query failed.
    QueryError { message: String },
    /// The edit batch commit outcome.
    CommitResult { ok: bool, message: String },
}

impl From<ResultsAction> for Action {
    fn from(a: ResultsAction) -> Self {
        // Action-to-message routing for SQL actions is handled in the loop
        // (sql_action_to_msg); this arm is only a compile-time requirement of
        // the `ErasedEffect` wrapper and is not reached for the variants that
        // carry tab context.
        match a {
            ResultsAction::ResultReady { .. }
            | ResultsAction::QueryError { .. }
            | ResultsAction::CommitResult { .. } => unreachable!(
                "ResultsAction is routed by sql_action_to_msg, not via Into<Action>"
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ResultsEffect {
    /// Run a SQL query and feed the result back.
    RunQuery {
        instance: String,
        connection: String,
        database: Option<String>,
        schema: String,
        sql: String,
        paginated: bool,
        page: usize,
        row_limit: usize,
    },
    /// Commit the edit batch. Executing requires a live connection pool; until
    /// the connection plumbing is wired this reports a deferred status.
    Commit { statements: Vec<String> },
    Detail(DetailEffect),
}

impl Effect for ResultsEffect {
    type Action = ResultsAction;

    fn run(self, _emit: Emitter<Self::Action>, services: std::sync::Arc<Services>) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                ResultsEffect::RunQuery {
                    instance,
                    connection,
                    database,
                    schema,
                    sql,
                    paginated,
                    page,
                    row_limit,
                } => {
                    let result = services
                        .execute_sql(
                            &instance,
                            &connection,
                            database.as_deref(),
                            &schema,
                            &sql,
                            paginated,
                            page,
                            row_limit,
                        )
                        .await;
                    match result {
                        Ok(q) => vec![ResultsAction::ResultReady {
                            result: q.into(),
                            paginated,
                        }],
                        Err(message) => vec![ResultsAction::QueryError { message }],
                    }
                }
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
