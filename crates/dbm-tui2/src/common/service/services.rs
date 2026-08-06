//! The service bundle injected into effects.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use dbm_core::{ConnectOpts, DatabaseDriver, SchemaIntrospector};
use dbm_driver_pg::PostgresDriver;
use dbm_store::Store;

/// Aggregated infrastructure dependencies available to effects.
///
/// Constructed once at the composition root and shared with the effect runner
/// via `Arc`. `Store` holds a `rusqlite::Connection` which is `Send` but not
/// `Sync`, so it is wrapped in `Arc<Mutex<Store>>` to be safely shared across
/// the concurrent effect tasks.
///
/// The database driver is stored as the concrete `PostgresDriver` here at the
/// composition root (the only engine today is PostgreSQL; the driver is a
/// zero-sized `Send + Sync` value). Features never touch the concrete driver:
/// they call the high-level catalog helpers below, so high-level modules depend
/// on the `Services` abstraction rather than on a driver implementation
/// (dependency inversion).
#[derive(Clone)]
pub struct Services {
    /// The discovery/session store (SQLite-backed).
    pub store: Arc<Mutex<Store>>,
    /// The database driver used for connection and metadata introspection.
    pub driver: Arc<PostgresDriver>,
}

impl Services {
    /// Create the service bundle with a default local store and the
    /// PostgreSQL driver. Called once at the composition root.
    pub fn new() -> anyhow::Result<Self> {
        let store = Store::open_default()?;
        Ok(Services {
            store: Arc::new(Mutex::new(store)),
            driver: Arc::new(PostgresDriver),
        })
    }

    /// Resolve a connection URL for `instance`/`connection`, optionally
    /// overriding the database (used to connect to a specific database for
    /// schema listing).
    pub async fn connection_url(
        &self,
        instance: &str,
        connection: &str,
        database: Option<&str>,
    ) -> Result<String, String> {
        let store = self.store.clone();
        let instance = instance.to_string();
        let connection = connection.to_string();
        let database = database.map(str::to_string);
        tokio::task::spawn_blocking(move || {
            let store = store.lock().expect("services store lock");
            match &database {
                Some(db) => store
                    .instance_connection_url_for_names(&instance, &connection, db)
                    .map_err(|e| e.to_string()),
                None => store
                    .instance_connection_url(&instance, &connection)
                    .map_err(|e| e.to_string()),
            }
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// Build a connection-test callback that pings the database driver.
    ///
    /// The returned closure matches the store's connection precheck signature:
    /// given a connection URL it connects and pings the server, returning the
    /// server version (or an error string). Features receive this through the
    /// `Services` abstraction rather than touching the concrete driver (DIP).
    pub fn connection_test_ping(
        &self,
    ) -> impl FnOnce(&str) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> + Send + use<> {
        let driver: Arc<PostgresDriver> = Arc::clone(&self.driver);
        move |url| {
            // Own the URL before the async block so the returned future is
            // `Send + 'static` (the store's ping callback requires it).
            let url = url.to_string();
            let driver = Arc::clone(&driver);
            Box::pin(async move {
                let pool = driver
                    .connect(&ConnectOpts::new(url))
                    .await
                    .map_err(|e| e.user_message().to_string())?;
                driver
                    .ping(&pool)
                    .await
                    .map_err(|e| e.user_message().to_string())
            })
        }
    }

    /// List the databases of a connection.
    pub async fn list_databases(&self, instance: &str, connection: &str) -> Result<Vec<String>, String> {
        let url = self.connection_url(instance, connection, None).await?;
        let pool = self
            .driver
            .connect(&ConnectOpts::new(url))
            .await
            .map_err(|e| e.user_message().to_string())?;
        self.driver
            .list_databases(&pool)
            .await
            .map_err(|e| e.user_message().to_string())
    }

    /// List the schemas of a specific database.
    pub async fn list_schemas(
        &self,
        instance: &str,
        connection: &str,
        database: &str,
    ) -> Result<Vec<String>, String> {
        let url = self.connection_url(instance, connection, Some(database)).await?;
        let pool = self
            .driver
            .connect(&ConnectOpts::new(url))
            .await
            .map_err(|e| e.user_message().to_string())?;
        self.driver
            .list_schemas(&pool)
            .await
            .map_err(|e| e.user_message().to_string())
    }

    /// Execute a SQL statement against a connection.
    ///
    /// Resolves the connection URL (optionally overriding the database), connects
    /// and runs the query — either plain (`execute_in_schema`) or paginated
    /// (`execute_paginated` with a `LIMIT`/`OFFSET`). A non-`SELECT` statement
    /// returns a result with zero columns and `rows_affected` set. Returns the
    /// raw driver result; callers project it into `QueryResultData`.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_sql(
        &self,
        instance: &str,
        connection: &str,
        database: Option<&str>,
        schema: &str,
        sql: &str,
        paginated: bool,
        page: usize,
        row_limit: usize,
    ) -> Result<dbm_core::QueryResult, String> {
        let url = self.connection_url(instance, connection, database).await?;
        let pool = self
            .driver
            .connect(&ConnectOpts::new(url))
            .await
            .map_err(|e| e.user_message().to_string())?;
        let result = if paginated {
            let offset = (page.saturating_sub(1) as u64) * row_limit as u64;
            self.driver
                .execute_paginated(&pool, schema, sql, row_limit as u64, offset)
                .await
        } else {
            self.driver.execute_in_schema(&pool, schema, sql).await
        };
        result.map_err(|e| e.user_message().to_string())
    }

    /// Count the rows a query would return (for pagination total).
    pub async fn count_rows(
        &self,
        instance: &str,
        connection: &str,
        database: Option<&str>,
        schema: &str,
        sql: &str,
    ) -> Result<Option<u64>, String> {
        let url = self.connection_url(instance, connection, database).await?;
        let pool = self
            .driver
            .connect(&ConnectOpts::new(url))
            .await
            .map_err(|e| e.user_message().to_string())?;
        self.driver
            .count_rows(&pool, schema, sql)
            .await
            .map_err(|e| e.user_message().to_string())
    }

    /// List the primary-key column names of a table (for row-edit gating).
    pub async fn list_primary_keys(
        &self,
        instance: &str,
        connection: &str,
        database: Option<&str>,
        schema: &str,
        table: &str,
    ) -> Result<Vec<String>, String> {
        let url = self.connection_url(instance, connection, database).await?;
        let pool = self
            .driver
            .connect(&ConnectOpts::new(url))
            .await
            .map_err(|e| e.user_message().to_string())?;
        self.driver
            .list_primary_keys(&pool, schema, table)
            .await
            .map_err(|e| e.user_message().to_string())
    }

    /// List the extensions of a specific database.
    pub async fn list_extensions(
        &self,
        instance: &str,
        connection: &str,
        database: &str,
    ) -> Result<Vec<String>, String> {
        let url = self.connection_url(instance, connection, Some(database)).await?;
        let pool = self
            .driver
            .connect(&ConnectOpts::new(url))
            .await
            .map_err(|e| e.user_message().to_string())?;
        self.driver
            .list_extensions(&pool)
            .await
            .map_err(|e| e.user_message().to_string())
    }

    /// List the tables of a schema within a specific database.
    pub async fn list_tables(
        &self,
        instance: &str,
        connection: &str,
        database: &str,
        schema: &str,
    ) -> Result<Vec<String>, String> {
        let url = self.connection_url(instance, connection, Some(database)).await?;
        let pool = self
            .driver
            .connect(&ConnectOpts::new(url))
            .await
            .map_err(|e| e.user_message().to_string())?;
        self.driver
            .list_tables(&pool, schema)
            .await
            .map_err(|e| e.user_message().to_string())
    }

    /// List the views of a schema within a specific database.
    pub async fn list_views(
        &self,
        instance: &str,
        connection: &str,
        database: &str,
        schema: &str,
    ) -> Result<Vec<String>, String> {
        let url = self.connection_url(instance, connection, Some(database)).await?;
        let pool = self
            .driver
            .connect(&ConnectOpts::new(url))
            .await
            .map_err(|e| e.user_message().to_string())?;
        self.driver
            .list_views(&pool, schema)
            .await
            .map_err(|e| e.user_message().to_string())
    }

    /// List the materialized views of a schema within a specific database.
    pub async fn list_matviews(
        &self,
        instance: &str,
        connection: &str,
        database: &str,
        schema: &str,
    ) -> Result<Vec<String>, String> {
        let url = self.connection_url(instance, connection, Some(database)).await?;
        let pool = self
            .driver
            .connect(&ConnectOpts::new(url))
            .await
            .map_err(|e| e.user_message().to_string())?;
        self.driver
            .list_matviews(&pool, schema)
            .await
            .map_err(|e| e.user_message().to_string())
    }

    /// List the procedures of a schema within a specific database.
    pub async fn list_procedures(
        &self,
        instance: &str,
        connection: &str,
        database: &str,
        schema: &str,
    ) -> Result<Vec<String>, String> {
        let url = self.connection_url(instance, connection, Some(database)).await?;
        let pool = self
            .driver
            .connect(&ConnectOpts::new(url))
            .await
            .map_err(|e| e.user_message().to_string())?;
        self.driver
            .list_procedures(&pool, schema)
            .await
            .map_err(|e| e.user_message().to_string())
    }

    /// List the functions of a schema within a specific database.
    pub async fn list_functions(
        &self,
        instance: &str,
        connection: &str,
        database: &str,
        schema: &str,
    ) -> Result<Vec<String>, String> {
        let url = self.connection_url(instance, connection, Some(database)).await?;
        let pool = self
            .driver
            .connect(&ConnectOpts::new(url))
            .await
            .map_err(|e| e.user_message().to_string())?;
        self.driver
            .list_functions(&pool, schema)
            .await
            .map_err(|e| e.user_message().to_string())
    }

    /// List the sequences of a schema within a specific database.
    pub async fn list_sequences(
        &self,
        instance: &str,
        connection: &str,
        database: &str,
        schema: &str,
    ) -> Result<Vec<String>, String> {
        let url = self.connection_url(instance, connection, Some(database)).await?;
        let pool = self
            .driver
            .connect(&ConnectOpts::new(url))
            .await
            .map_err(|e| e.user_message().to_string())?;
        self.driver
            .list_sequences(&pool, schema)
            .await
            .map_err(|e| e.user_message().to_string())
    }

    /// Execute a batch of DML statements inside a single transaction.
    ///
    /// Every `UPDATE` / `DELETE` must affect exactly one row; a mismatch is
    /// reported as a conflict and rolls the whole batch back. Returns the
    /// number of statements that were executed.
    pub async fn commit_batch(
        &self,
        instance: &str,
        connection: &str,
        database: Option<&str>,
        schema: &str,
        statements: &[String],
    ) -> Result<usize, String> {
        let url = self.connection_url(instance, connection, database).await?;
        let pool = self
            .driver
            .connect(&ConnectOpts::new(url))
            .await
            .map_err(|e| e.user_message().to_string())?;
        let stmt_kinds: Vec<bool> = statements
            .iter()
            .map(|s| crate::common::utils::sql_editability::statement_requires_one_row(s))
            .collect();
        let driver = self.driver.clone();
        driver
            .run_in_transaction(&pool, schema, statements, move |index, affected| {
                if stmt_kinds.get(index).copied().unwrap_or(false) && affected != 1 {
                    return Err(dbm_core::ApplicationError::Database {
                        message: format!("conflict: statement {} affected {affected} row(s)", index + 1),
                        severity: dbm_core::ErrorSeverity::Error,
                    });
                }
                Ok(())
            })
            .await
            .map(|_| statements.len())
            .map_err(|e| e.user_message().to_string())
    }
}

impl Default for Services {
    fn default() -> Self {
        // In-memory store for tests / when no disk store is wanted. Falls back
        // to a fresh in-memory store if the default location is unavailable.
        let store = Store::open_in_memory()
            .or_else(|_| Store::open_default())
            .expect("failed to open an in-memory store");
        Services {
            store: Arc::new(Mutex::new(store)),
            driver: Arc::new(PostgresDriver),
        }
    }
}
