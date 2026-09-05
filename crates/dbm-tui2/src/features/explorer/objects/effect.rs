//! Explorer objects feature effects and actions.

use std::sync::Arc;

use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;

use super::state::ObjectKind;

/// Actions produced by objects effects.
#[derive(Debug, Clone)]
pub enum ObjectsAction {
    /// The databases of the bound connection were loaded.
    DatabasesLoaded { databases: Vec<String> },
    /// Loading databases failed.
    DatabasesError { error: String },
    /// The schemas of a database were loaded.
    SchemasLoaded {
        database: String,
        schemas: Vec<String>,
    },
    /// Loading schemas failed.
    SchemasError { database: String, error: String },
    /// The extensions of a database were loaded.
    ExtensionsLoaded {
        database: String,
        extensions: Vec<String>,
    },
    /// Loading extensions failed.
    ExtensionsError { database: String, error: String },
    /// A schema-scoped object list was loaded.
    ObjectListLoaded {
        database: String,
        schema: String,
        kind: ObjectKind,
        items: Vec<String>,
    },
    /// Loading a schema-scoped object list failed.
    ObjectListError {
        database: String,
        schema: String,
        kind: ObjectKind,
        error: String,
    },
}

/// Effects emitted by the objects tree: each lazily fetches one level of the
/// catalog for the bound connection through `Services`.
#[derive(Debug, Clone)]
pub enum ObjectsEffect {
    /// Load the databases of the bound connection.
    LoadDatabases {
        instance: String,
        connection: String,
    },
    /// Load the schemas of a database.
    LoadSchemas {
        instance: String,
        connection: String,
        database: String,
    },
    /// Load the extensions of a database.
    LoadExtensions {
        instance: String,
        connection: String,
        database: String,
    },
    /// Load one schema-scoped object list.
    LoadObjectList {
        instance: String,
        connection: String,
        database: String,
        schema: String,
        kind: ObjectKind,
    },
}

impl Effect for ObjectsEffect {
    type Action = ObjectsAction;

    fn run(
        self,
        _emit: Emitter<Self::Action>,
        services: Arc<Services>,
    ) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                ObjectsEffect::LoadDatabases {
                    instance,
                    connection,
                } => match services.list_databases(&instance, &connection).await {
                    Ok(databases) => vec![ObjectsAction::DatabasesLoaded { databases }],
                    Err(error) => vec![ObjectsAction::DatabasesError { error }],
                },
                ObjectsEffect::LoadSchemas {
                    instance,
                    connection,
                    database,
                } => {
                    match services
                        .list_schemas(&instance, &connection, &database)
                        .await
                    {
                        Ok(schemas) => vec![ObjectsAction::SchemasLoaded { database, schemas }],
                        Err(error) => vec![ObjectsAction::SchemasError { database, error }],
                    }
                }
                ObjectsEffect::LoadExtensions {
                    instance,
                    connection,
                    database,
                } => {
                    match services
                        .list_extensions(&instance, &connection, &database)
                        .await
                    {
                        Ok(extensions) => {
                            vec![ObjectsAction::ExtensionsLoaded {
                                database,
                                extensions,
                            }]
                        }
                        Err(error) => vec![ObjectsAction::ExtensionsError { database, error }],
                    }
                }
                ObjectsEffect::LoadObjectList {
                    instance,
                    connection,
                    database,
                    schema,
                    kind,
                } => {
                    let result = list_object_kind(
                        &services,
                        &instance,
                        &connection,
                        &database,
                        &schema,
                        kind,
                    )
                    .await;
                    match result {
                        Ok(items) => vec![ObjectsAction::ObjectListLoaded {
                            database,
                            schema,
                            kind,
                            items,
                        }],
                        Err(error) => vec![ObjectsAction::ObjectListError {
                            database,
                            schema,
                            kind,
                            error,
                        }],
                    }
                }
            }
        })
    }
}

async fn list_object_kind(
    services: &Services,
    instance: &str,
    connection: &str,
    database: &str,
    schema: &str,
    kind: ObjectKind,
) -> Result<Vec<String>, String> {
    match kind {
        ObjectKind::Tables => {
            services
                .list_tables(instance, connection, database, schema)
                .await
        }
        ObjectKind::Views => {
            services
                .list_views(instance, connection, database, schema)
                .await
        }
        ObjectKind::Matviews => {
            services
                .list_matviews(instance, connection, database, schema)
                .await
        }
        ObjectKind::Procedures => {
            services
                .list_procedures(instance, connection, database, schema)
                .await
        }
        ObjectKind::Functions => {
            services
                .list_functions(instance, connection, database, schema)
                .await
        }
        ObjectKind::Sequences => {
            services
                .list_sequences(instance, connection, database, schema)
                .await
        }
        ObjectKind::Extensions => {
            // Extensions are database-scoped and fetched via LoadExtensions.
            Err("extensions are not schema-scoped".to_string())
        }
    }
}
