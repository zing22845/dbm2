//! Results feature effects and actions.
//!
//! The side effects are running a SQL query (via `Services::execute_sql`),
//! resolving the result's editability (via `Services::list_primary_keys`) and
//! committing an edit batch (via `Services::commit_batch`).

use super::detail::effect::DetailEffect;
use super::edit_sql::EditTarget;
use super::list::effect::ListEffect;
use super::state::QueryResultData;
use crate::app::action::Action;
use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;
use crate::common::utils::sql_editability::{
    EditabilityReason, all_primary_keys_present, analyze_editable_query_editability,
    editability_reason_message,
};
use crate::features::sql_workspace::effect::SqlAction;
use crate::features::sql_workspace::sql_tab::effect::SqlTabAction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsAction {
    /// A query completed with a result.
    ResultReady {
        result: QueryResultData,
        paginated: bool,
    },
    /// A query failed.
    QueryError { message: String },
    /// A COUNT total-rows request finished (`sql` guards against applying a
    /// stale total after the query changed; `None` when the query is not
    /// count-able or the count failed).
    CountReady { sql: String, total: Option<u64> },
    /// The edit batch commit outcome.
    CommitResult { ok: bool, message: String },
    /// The editability of the result was resolved.
    EditabilityReady {
        target: Option<EditTarget>,
        blocked: Option<String>,
    },
}

impl From<ResultsAction> for Action {
    fn from(a: ResultsAction) -> Self {
        // Action-to-message routing for SQL actions is handled in the loop
        // (`sql_action_to_msg`), which preserves the originating `tab_id`. This
        // conversion is a compile-time requirement of the `ErasedEffect`
        // wrapper for streamed emission; it carries no tab context, so it uses
        // tab 0 as a safe default (the real routed path never goes through
        // here, so tab 0 is only reached by `sql_action_to_msg` for a streamed
        // child action).
        Action::Sql(SqlAction::SqlTab(SqlTabAction::Results {
            tab_id: 0,
            action: a,
        }))
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
    /// Commit the edit batch inside a single transaction against the tab's
    /// connection. A conflict (an `UPDATE`/`DELETE` that affects != 1 row)
    /// rolls the batch back.
    Commit {
        instance: String,
        connection: String,
        database: Option<String>,
        schema: String,
        statements: Vec<String>,
    },
    /// Resolve whether the last result can be edited: run the pure SQL
    /// editability analysis, then (if structurally editable) confirm the
    /// primary keys are present via `Services::list_primary_keys`.
    CheckEditability {
        instance: String,
        connection: String,
        database: Option<String>,
        schema: String,
        sql: String,
        result_columns: Vec<String>,
    },
    /// Count the total rows of `sql` (COUNT over the query) and feed it back.
    CountRows {
        instance: String,
        connection: String,
        database: Option<String>,
        schema: String,
        sql: String,
    },
    Detail(DetailEffect),
    List(ListEffect),
    /// Sync the viewport dimensions after render (pure state update).
    SyncViewport {
        rows: usize,
        width: u16,
    },
}

impl Effect for ResultsEffect {
    type Action = ResultsAction;

    fn run(
        self,
        _emit: Emitter<Self::Action>,
        services: std::sync::Arc<Services>,
    ) -> BoxFuture<Vec<Self::Action>> {
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
                ResultsEffect::Commit {
                    instance,
                    connection,
                    database,
                    schema,
                    statements,
                } => {
                    let outcome = services
                        .commit_batch(
                            &instance,
                            &connection,
                            database.as_deref(),
                            &schema,
                            &statements,
                        )
                        .await;
                    match outcome {
                        Ok(n) => vec![ResultsAction::CommitResult {
                            ok: true,
                            message: format!("Committed {n} statement(s)"),
                        }],
                        Err(message) => vec![ResultsAction::CommitResult { ok: false, message }],
                    }
                }
                ResultsEffect::CheckEditability {
                    instance,
                    connection,
                    database,
                    schema,
                    sql,
                    result_columns,
                } => {
                    let col_refs: Vec<&str> = result_columns.iter().map(|s| s.as_str()).collect();
                    // Structural analysis first (pure, no DB round-trip).
                    let analysis = analyze_editable_query_editability(&sql);
                    if !analysis.editable {
                        let reason = analysis.reason.unwrap_or(EditabilityReason::ComplexSource);
                        return vec![ResultsAction::EditabilityReady {
                            target: None,
                            blocked: Some(editability_reason_message(reason)),
                        }];
                    }
                    let Some(info) = analysis.analysis else {
                        return vec![ResultsAction::EditabilityReady {
                            target: None,
                            blocked: Some(editability_reason_message(
                                EditabilityReason::MetadataUnavailable,
                            )),
                        }];
                    };
                    let schema = info
                        .schema
                        .clone()
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| schema.clone());
                    let table = info.table_name.clone();
                    if schema.is_empty() || table.is_empty() {
                        return vec![ResultsAction::EditabilityReady {
                            target: None,
                            blocked: Some(editability_reason_message(
                                EditabilityReason::MetadataUnavailable,
                            )),
                        }];
                    }
                    let pks = match services
                        .list_primary_keys(
                            &instance,
                            &connection,
                            database.as_deref(),
                            &schema,
                            &table,
                        )
                        .await
                    {
                        Ok(pks) => pks,
                        Err(e) => {
                            return vec![ResultsAction::EditabilityReady {
                                target: None,
                                blocked: Some(format!(
                                    "{}: {e}",
                                    editability_reason_message(
                                        EditabilityReason::MetadataUnavailable
                                    )
                                )),
                            }];
                        }
                    };
                    if pks.is_empty() {
                        return vec![ResultsAction::EditabilityReady {
                            target: None,
                            blocked: Some(editability_reason_message(
                                EditabilityReason::NoPrimaryKey,
                            )),
                        }];
                    }
                    if !all_primary_keys_present(&pks, &col_refs) {
                        let missing: Vec<String> = pks
                            .iter()
                            .filter(|pk| !col_refs.iter().any(|c| c.eq_ignore_ascii_case(pk)))
                            .cloned()
                            .collect();
                        return vec![ResultsAction::EditabilityReady {
                            target: None,
                            blocked: Some(format!(
                                "{}: missing {}",
                                editability_reason_message(
                                    EditabilityReason::PrimaryKeyNotReturned
                                ),
                                missing.join(", ")
                            )),
                        }];
                    }
                    vec![ResultsAction::EditabilityReady {
                        target: Some(EditTarget {
                            schema,
                            table,
                            primary_keys: pks,
                            columns: result_columns,
                        }),
                        blocked: None,
                    }]
                }
                ResultsEffect::CountRows {
                    instance,
                    connection,
                    database,
                    schema,
                    sql,
                } => {
                    let total = services
                        .count_rows(&instance, &connection, database.as_deref(), &schema, &sql)
                        .await
                        .ok()
                        .flatten();
                    vec![ResultsAction::CountReady { sql, total }]
                }
                ResultsEffect::Detail(_) => Vec::new(),
                ResultsEffect::List(_) => Vec::new(),
                ResultsEffect::SyncViewport { .. } => Vec::new(),
            }
        })
    }
}
