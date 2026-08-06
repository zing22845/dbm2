//! Editor feature effects and actions.

use std::collections::HashMap;

use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;
use super::context_picker::effect::{ContextPickerAction, ContextPickerEffect};
use super::sql_completion::effect::SqlCompletionEffect;

/// The SQL-completion catalog loaded for a tab's connection/schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionCatalogData {
    pub tables: Vec<String>,
    pub columns_by_table: HashMap<String, Vec<super::sql_completion::provider::ColumnInfo>>,
}

/// Actions produced by editor effects (from its child sub-modules).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorAction {
    /// An action from the context picker sub-module.
    ContextPicker(ContextPickerAction),
    /// The completion catalog for the tab's connection/schema was loaded.
    CompletionCatalogLoaded(CompletionCatalogData),
}

impl From<ContextPickerAction> for EditorAction {
    fn from(a: ContextPickerAction) -> Self {
        EditorAction::ContextPicker(a)
    }
}

impl From<super::sql_completion::effect::SqlCompletionAction> for EditorAction {
    fn from(_a: super::sql_completion::effect::SqlCompletionAction) -> Self {
        match _a {}
    }
}

/// Effects emitted by the editor feature, delegating to its child sub-modules.
#[derive(Debug, Clone)]
pub enum EditorEffect {
    /// An effect from the context picker sub-module.
    ContextPicker(ContextPickerEffect),
    /// An effect from the sql completion sub-module.
    SqlCompletion(SqlCompletionEffect),
    /// Load the SQL-completion catalog for a tab's connection/schema.
    LoadCompletionCatalog {
        instance: String,
        connection: String,
        database: Option<String>,
        schema: String,
    },
}

impl Effect for EditorEffect {
    type Action = EditorAction;

    fn run(self, emit: Emitter<Self::Action>, services: std::sync::Arc<Services>) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                EditorEffect::ContextPicker(e) => {
                    let emit = emit.map::<ContextPickerAction>();
                    e.run(emit, services)
                        .await
                        .into_iter()
                        .map(Into::into)
                        .collect()
                }
                EditorEffect::SqlCompletion(e) => {
                    let emit = emit.map::<super::sql_completion::effect::SqlCompletionAction>();
                    e.run(emit, services)
                        .await
                        .into_iter()
                        .map(Into::into)
                        .collect()
                }
                EditorEffect::LoadCompletionCatalog {
                    instance,
                    connection,
                    database,
                    schema,
                } => {
                    match services
                        .completion_catalog(
                            &instance,
                            &connection,
                            database.as_deref(),
                            &schema,
                        )
                        .await
                    {
                        Ok((tables, columns_by_table)) => {
                            let columns_by_table = columns_by_table
                                .into_iter()
                                .map(|(name, cols)| {
                                    (
                                        name,
                                        cols.into_iter().map(Into::into).collect(),
                                    )
                                })
                                .collect();
                            vec![EditorAction::CompletionCatalogLoaded(
                                CompletionCatalogData {
                                    tables,
                                    columns_by_table,
                                },
                            )]
                        }
                        Err(error) => {
                            tracing::warn!("completion catalog load failed: {error}");
                            Vec::new()
                        }
                    }
                }
            }
        })
    }
}
