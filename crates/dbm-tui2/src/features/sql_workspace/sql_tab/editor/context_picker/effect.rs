//! Context picker sub-module effects and actions.
//!
//! The only side effects are catalog fetches: listing the databases of a
//! connection and the schemas of a (previewed) database. These delegate to the
//! `Services` catalog helpers (which resolve the URL, connect and introspect
//! behind the service abstraction), so this module never touches a driver.

use std::sync::Arc;

use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;

/// Actions produced by context picker effects, fed back as messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextPickerAction {
    /// The databases list for the connection was loaded.
    DatabasesLoaded { items: Vec<String> },
    /// Loading databases failed.
    DatabasesError { error: String },
    /// The schemas list for the previewed database was loaded.
    SchemasLoaded { items: Vec<String> },
    /// Loading schemas failed.
    SchemasError { error: String },
}

/// Effects emitted by the context picker.
#[derive(Debug, Clone)]
pub enum ContextPickerEffect {
    /// Load the databases list for a connection.
    LoadDatabases { instance: String, connection: String },
    /// Load the schemas list for a specific database.
    LoadSchemas { instance: String, connection: String, database: String },
}

impl Effect for ContextPickerEffect {
    type Action = ContextPickerAction;

    fn run(self, _emit: Emitter<Self::Action>, services: Arc<Services>) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                ContextPickerEffect::LoadDatabases { instance, connection } => {
                    match services.list_databases(&instance, &connection).await {
                        Ok(items) => vec![ContextPickerAction::DatabasesLoaded { items }],
                        Err(error) => vec![ContextPickerAction::DatabasesError { error }],
                    }
                }
                ContextPickerEffect::LoadSchemas { instance, connection, database } => {
                    match services.list_schemas(&instance, &connection, &database).await {
                        Ok(items) => vec![ContextPickerAction::SchemasLoaded { items }],
                        Err(error) => vec![ContextPickerAction::SchemasError { error }],
                    }
                }
            }
        })
    }
}
