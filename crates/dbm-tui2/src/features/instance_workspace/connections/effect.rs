//! Instance connections feature effects and actions.

use std::sync::Arc;

use dbm_store::{NewInstanceConnection, UpdateInstanceConnection};

use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;

/// Actions produced by connections effects.
#[derive(Debug, Clone)]
pub enum ConnectionsAction {
    /// The store returned the connections.
    Loaded {
        connections: Vec<dbm_store::InstanceConnection>,
    },
    /// A connection was saved (added or edited).
    Saved,
    /// A connection was deleted.
    Deleted,
    /// The operation failed.
    Error { error: String },
    /// A form test completed: `ok` reports whether the connection reached the
    /// database, `error` carries the reason when it did not.
    TestResult { ok: bool, error: Option<String> },
    /// A list test completed: same as `TestResult`, but the list must reload so
    /// the row's test timestamps and whole-row color update.
    ListTestResult { ok: bool, error: Option<String> },
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
    DeleteConnection {
        instance_name: String,
        connection_name: String,
    },
    /// Test the form's current values against the instance (ping), without
    /// saving (the form's `t` action).
    TestFormConnection {
        instance_name: String,
        connection: NewInstanceConnection,
    },
    /// Test an edited connection's current form values, borrowing the stored
    /// password when the password field is blank (the form's `t` on an edit
    /// form). Unlike [`ConnectionsEffect::TestConnection`], this uses the form's
    /// modified name/username/database so edited fields are actually exercised.
    TestEditedFormConnection {
        instance_name: String,
        original_name: String,
        connection: NewInstanceConnection,
    },
    /// Test a saved connection from the list (the list's `t` action), recording
    /// the outcome timestamp.
    TestConnection {
        instance_name: String,
        connection_name: String,
    },
}

impl Effect for ConnectionsEffect {
    type Action = ConnectionsAction;

    fn run(
        self,
        _emit: Emitter<Self::Action>,
        services: Arc<Services>,
    ) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            let store = services.store.clone();
            match self {
                ConnectionsEffect::LoadConnections { instance_name } => {
                    let result = tokio::task::spawn_blocking(move || {
                        store
                            .lock()
                            .expect("iw store lock")
                            .list_instance_connections(&instance_name)
                    })
                    .await;
                    match result {
                        Ok(Ok(connections)) => vec![ConnectionsAction::Loaded { connections }],
                        Ok(Err(e)) => vec![ConnectionsAction::Error {
                            error: e.to_string(),
                        }],
                        Err(e) => vec![ConnectionsAction::Error {
                            error: e.to_string(),
                        }],
                    }
                }
                ConnectionsEffect::AddConnection {
                    instance_name,
                    connection,
                } => {
                    // The store's precheck pings the real server via the driver
                    // before persisting, so an unreachable database fails the
                    // save instead of silently succeeding.
                    let ping = services.connection_test_ping();
                    let result = tokio::task::spawn_blocking(move || {
                        store
                            .lock()
                            .expect("iw store lock")
                            .add_instance_connection(&instance_name, connection, ping)
                    })
                    .await;
                    match result {
                        Ok(Ok(_)) => vec![ConnectionsAction::Saved],
                        Ok(Err(e)) => vec![ConnectionsAction::Error {
                            error: e.to_string(),
                        }],
                        Err(e) => vec![ConnectionsAction::Error {
                            error: e.to_string(),
                        }],
                    }
                }
                ConnectionsEffect::EditConnection {
                    instance_name,
                    original_name,
                    connection,
                } => {
                    let patch = edit_connection_patch(connection);
                    let ping = services.connection_test_ping();
                    let result = tokio::task::spawn_blocking(move || {
                        store
                            .lock()
                            .expect("iw store lock")
                            .update_instance_connection(&instance_name, &original_name, patch, ping)
                    })
                    .await;
                    match result {
                        Ok(Ok(_)) => vec![ConnectionsAction::Saved],
                        Ok(Err(e)) => vec![ConnectionsAction::Error {
                            error: e.to_string(),
                        }],
                        Err(e) => vec![ConnectionsAction::Error {
                            error: e.to_string(),
                        }],
                    }
                }
                ConnectionsEffect::DeleteConnection {
                    instance_name,
                    connection_name,
                } => {
                    let result = tokio::task::spawn_blocking(move || {
                        store
                            .lock()
                            .expect("iw store lock")
                            .delete_instance_connection(&instance_name, &connection_name)
                    })
                    .await;
                    match result {
                        Ok(Ok(_)) => vec![ConnectionsAction::Deleted],
                        Ok(Err(e)) => vec![ConnectionsAction::Error {
                            error: e.to_string(),
                        }],
                        Err(e) => vec![ConnectionsAction::Error {
                            error: e.to_string(),
                        }],
                    }
                }
                ConnectionsEffect::TestFormConnection {
                    instance_name,
                    connection,
                } => {
                    // Ping the database with the form's current values; nothing
                    // is saved. A store error or any error-level precheck issue
                    // means the connection did not reach the database.
                    let ping = services.connection_test_ping();
                    let result = tokio::task::spawn_blocking(move || {
                        store
                            .lock()
                            .expect("iw store lock")
                            .test_instance_connection(&instance_name, &connection, ping)
                    })
                    .await;
                    match result {
                        Ok(Ok(precheck)) => {
                            let error = precheck
                                .issues
                                .iter()
                                .find(|i| i.level == dbm_store::PrecheckLevel::Error)
                                .map(|i| i.message.clone());
                            vec![ConnectionsAction::TestResult {
                                ok: error.is_none(),
                                error,
                            }]
                        }
                        Ok(Err(e)) => vec![ConnectionsAction::TestResult {
                            ok: false,
                            error: Some(e.to_string()),
                        }],
                        Err(e) => vec![ConnectionsAction::TestResult {
                            ok: false,
                            error: Some(e.to_string()),
                        }],
                    }
                }
                ConnectionsEffect::TestEditedFormConnection {
                    instance_name,
                    original_name,
                    connection,
                } => {
                    // Ping with the form's current name/username/database but
                    // borrow the stored password when the password field is blank.
                    let ping = services.connection_test_ping();
                    let result = tokio::task::spawn_blocking(move || {
                        let store = store.lock().expect("iw store lock");
                        store.test_edited_instance_connection(
                            &instance_name,
                            &original_name,
                            &connection,
                            ping,
                        )
                    })
                    .await;
                    match result {
                        Ok(Ok(precheck)) => {
                            let error = precheck
                                .issues
                                .iter()
                                .find(|i| i.level == dbm_store::PrecheckLevel::Error)
                                .map(|i| i.message.clone());
                            vec![ConnectionsAction::TestResult {
                                ok: error.is_none(),
                                error,
                            }]
                        }
                        Ok(Err(e)) => vec![ConnectionsAction::TestResult {
                            ok: false,
                            error: Some(e.to_string()),
                        }],
                        Err(e) => vec![ConnectionsAction::TestResult {
                            ok: false,
                            error: Some(e.to_string()),
                        }],
                    }
                }
                ConnectionsEffect::TestConnection {
                    instance_name,
                    connection_name,
                } => {
                    // Test a saved connection with its stored credentials and
                    // record the outcome timestamp on the row.
                    let ping = services.connection_test_ping();
                    let result = tokio::task::spawn_blocking(
                        move || -> dbm_store::StoreResult<dbm_store::ConnectionPrecheck> {
                            let store = store.lock().expect("iw store lock");
                            let precheck = store.test_saved_instance_connection(
                                &instance_name,
                                &connection_name,
                                ping,
                            )?;
                            store.record_connection_test_result(
                                &instance_name,
                                &connection_name,
                                precheck.ok,
                            )?;
                            Ok(precheck)
                        },
                    )
                    .await;
                    match result {
                        Ok(Ok(precheck)) => {
                            let error = precheck
                                .issues
                                .iter()
                                .find(|i| i.level == dbm_store::PrecheckLevel::Error)
                                .map(|i| i.message.clone());
                            vec![ConnectionsAction::ListTestResult {
                                ok: error.is_none(),
                                error,
                            }]
                        }
                        Ok(Err(e)) => vec![ConnectionsAction::ListTestResult {
                            ok: false,
                            error: Some(e.to_string()),
                        }],
                        Err(e) => vec![ConnectionsAction::ListTestResult {
                            ok: false,
                            error: Some(e.to_string()),
                        }],
                    }
                }
            }
        })
    }
}

/// Build the update patch for an edited connection.
///
/// Every field the form carries is patched — `ssl_mode` included, which must be
/// forwarded or an edit would silently keep the stored mode — while a blank
/// password keeps the stored one.
fn edit_connection_patch(connection: NewInstanceConnection) -> UpdateInstanceConnection {
    UpdateInstanceConnection {
        name: Some(connection.name),
        username: Some(connection.username),
        database: Some(connection.database),
        password: match connection.password {
            Some(p) if !p.is_empty() => Some(Some(p)),
            _ => None,
        },
        ssl_mode: connection.ssl_mode,
        env_label: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_patch_forwards_the_selected_ssl_mode() {
        let patch = edit_connection_patch(NewInstanceConnection {
            name: "conn".into(),
            username: "u".into(),
            database: "postgres".into(),
            password: None,
            ssl_mode: Some("verify-full".into()),
            env_label: None,
        });
        assert_eq!(patch.ssl_mode.as_deref(), Some("verify-full"));
        assert_eq!(patch.name.as_deref(), Some("conn"));
        // A blank password must keep the stored one.
        assert!(patch.password.is_none());
    }

    #[test]
    fn edit_patch_carries_password_but_keeps_an_absent_ssl_mode() {
        let patch = edit_connection_patch(NewInstanceConnection {
            name: "conn".into(),
            username: "u".into(),
            database: "postgres".into(),
            password: Some("secret".into()),
            ssl_mode: None,
            env_label: None,
        });
        assert_eq!(patch.password, Some(Some("secret".to_string())));
        assert!(patch.ssl_mode.is_none());
    }
}
