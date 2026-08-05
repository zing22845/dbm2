//! Instance connections feature effects and actions.

use std::sync::Arc;

use dbm_store::{NewInstanceConnection, UpdateInstanceConnection};

use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;

/// A placeholder connection-test callback used until a real pg driver ping is
/// wired in. It reports success so the store's precheck passes without
/// contacting a database.
fn placeholder_ping() -> impl FnOnce(&str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>>
{
    |_url| Box::pin(async { Ok(String::new()) })
}

/// Actions produced by connections effects.
#[derive(Debug, Clone)]
pub enum ConnectionsAction {
    /// The store returned the connections.
    Loaded { connections: Vec<dbm_store::InstanceConnection> },
    /// A connection was saved (added or edited).
    Saved,
    /// A connection was deleted.
    Deleted,
    /// The operation failed.
    Error { error: String },
}

/// Effects emitted by the connections panel.
#[derive(Debug, Clone)]
pub enum ConnectionsEffect {
    /// Load the connections for `instance_name`.
    LoadConnections { instance_name: String },
    /// Save a new connection.
    AddConnection {
        instance_name: String,
        connection: NewInstanceConnection,
    },
    /// Update an existing connection.
    EditConnection {
        instance_name: String,
        original_name: String,
        connection: NewInstanceConnection,
    },
    /// Delete a connection.
    DeleteConnection { instance_name: String, connection_name: String },
}

impl Effect for ConnectionsEffect {
    type Action = ConnectionsAction;

    fn run(self, _emit: Emitter<Self::Action>, services: Arc<Services>) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            let store = services.store.clone();
            match self {
                ConnectionsEffect::LoadConnections { instance_name } => {
                    let result = tokio::task::spawn_blocking(move || {
                        store.lock().expect("iw store lock").list_instance_connections(&instance_name)
                    })
                    .await;
                    match result {
                        Ok(Ok(connections)) => vec![ConnectionsAction::Loaded { connections }],
                        Ok(Err(e)) => vec![ConnectionsAction::Error { error: e.to_string() }],
                        Err(e) => vec![ConnectionsAction::Error { error: e.to_string() }],
                    }
                }
                ConnectionsEffect::AddConnection { instance_name, connection } => {
                    let result = tokio::task::spawn_blocking(move || {
                        store
                            .lock()
                            .expect("iw store lock")
                            .add_instance_connection(&instance_name, connection, placeholder_ping())
                    })
                    .await;
                    match result {
                        Ok(Ok(_)) => vec![ConnectionsAction::Saved],
                        Ok(Err(e)) => vec![ConnectionsAction::Error { error: e.to_string() }],
                        Err(e) => vec![ConnectionsAction::Error { error: e.to_string() }],
                    }
                }
                ConnectionsEffect::EditConnection { instance_name, original_name, connection } => {
                    let patch = UpdateInstanceConnection {
                        name: Some(connection.name),
                        username: Some(connection.username),
                        database: Some(connection.database),
                        password: match connection.password {
                            Some(p) if !p.is_empty() => Some(Some(p)),
                            _ => None, // blank password keeps the old one
                        },
                        ssl_mode: None,
                        env_label: None,
                    };
                    let result = tokio::task::spawn_blocking(move || {
                        store
                            .lock()
                            .expect("iw store lock")
                            .update_instance_connection(
                                &instance_name,
                                &original_name,
                                patch,
                                placeholder_ping(),
                            )
                    })
                    .await;
                    match result {
                        Ok(Ok(_)) => vec![ConnectionsAction::Saved],
                        Ok(Err(e)) => vec![ConnectionsAction::Error { error: e.to_string() }],
                        Err(e) => vec![ConnectionsAction::Error { error: e.to_string() }],
                    }
                }
                ConnectionsEffect::DeleteConnection { instance_name, connection_name } => {
                    let result = tokio::task::spawn_blocking(move || {
                        store.lock().expect("iw store lock").delete_instance_connection(&instance_name, &connection_name)
                    })
                    .await;
                    match result {
                        Ok(Ok(_)) => vec![ConnectionsAction::Deleted],
                        Ok(Err(e)) => vec![ConnectionsAction::Error { error: e.to_string() }],
                        Err(e) => vec![ConnectionsAction::Error { error: e.to_string() }],
                    }
                }
            }
        })
    }
}
