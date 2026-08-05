use std::any::Any;
use std::sync::Arc;

use async_trait::async_trait;

use crate::{ApplicationError, ColumnMeta, ConnectOpts, Engine, QueryResult};

/// Opaque connection pool handle.
///
/// Each driver wraps its concrete pool type in this handle via
/// [`ConnectionPool::new`]. The actual pool type is only accessible
/// inside the driver that created it, via [`ConnectionPool::downcast_ref`].
pub struct ConnectionPool {
    engine: Engine,
    inner: Arc<dyn Any + Send + Sync>,
}

impl Clone for ConnectionPool {
    fn clone(&self) -> Self {
        Self {
            engine: self.engine,
            inner: Arc::clone(&self.inner),
        }
    }
}

impl ConnectionPool {
    pub fn new<T: Send + Sync + 'static>(engine: Engine, inner: T) -> Self {
        Self {
            engine,
            inner: Arc::new(inner),
        }
    }

    pub fn engine(&self) -> Engine {
        self.engine
    }

    pub fn downcast_ref<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.inner.downcast_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_pool_downcast_works() {
        let pool = ConnectionPool::new(Engine::Postgres, 42u32);
        assert_eq!(pool.engine(), Engine::Postgres);
        assert_eq!(pool.downcast_ref::<u32>(), Some(&42));
        assert_eq!(pool.downcast_ref::<String>(), None);
    }

    #[test]
    fn connection_pool_clone_is_cheap() {
        let pool = ConnectionPool::new(Engine::Postgres, "test".to_string());
        let cloned = pool.clone();
        assert_eq!(cloned.engine(), Engine::Postgres);
        assert_eq!(cloned.downcast_ref::<String>(), Some(&"test".to_string()));
    }
}

/// Database driver trait — connection management, query execution, and transactions.
///
/// One implementation per database engine. Currently only PostgreSQL is supported.
#[async_trait]
pub trait DatabaseDriver: Send + Sync {
    fn engine(&self) -> Engine;
    fn display_name(&self) -> &'static str;

    async fn connect(&self, opts: &ConnectOpts) -> std::result::Result<ConnectionPool, ApplicationError>;

    async fn ping(&self, pool: &ConnectionPool) -> std::result::Result<String, ApplicationError>;

    async fn execute_query(
        &self,
        pool: &ConnectionPool,
        sql: &str,
    ) -> std::result::Result<QueryResult, ApplicationError>;

    async fn execute_in_schema(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        sql: &str,
    ) -> std::result::Result<QueryResult, ApplicationError>;

    async fn execute_paginated(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        sql: &str,
        limit: u64,
        offset: u64,
    ) -> std::result::Result<QueryResult, ApplicationError>;

    async fn has_rows_after(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        sql: &str,
        offset: u64,
    ) -> std::result::Result<bool, ApplicationError>;

    async fn count_rows(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        sql: &str,
    ) -> std::result::Result<Option<u64>, ApplicationError>;

    async fn set_search_path(
        &self,
        pool: &ConnectionPool,
        schema: &str,
    ) -> std::result::Result<(), ApplicationError>;

    async fn run_in_transaction<F>(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        statements: &[String],
        after_each: F,
    ) -> std::result::Result<Vec<u64>, ApplicationError>
    where
        F: FnMut(usize, u64) -> std::result::Result<(), ApplicationError> + Send + Sync + 'static;
}

/// Schema introspection trait — browse database structure.
///
/// Split from [`DatabaseDriver`] because schema browsing has
/// significantly different concerns and call patterns.
#[async_trait]
pub trait SchemaIntrospector: Send + Sync {
    async fn list_databases(&self, pool: &ConnectionPool) -> std::result::Result<Vec<String>, ApplicationError>;

    async fn list_schemas(&self, pool: &ConnectionPool) -> std::result::Result<Vec<String>, ApplicationError>;

    async fn list_tables(
        &self,
        pool: &ConnectionPool,
        schema: &str,
    ) -> std::result::Result<Vec<String>, ApplicationError>;

    async fn list_views(
        &self,
        pool: &ConnectionPool,
        schema: &str,
    ) -> std::result::Result<Vec<String>, ApplicationError>;

    async fn list_matviews(
        &self,
        pool: &ConnectionPool,
        schema: &str,
    ) -> std::result::Result<Vec<String>, ApplicationError>;

    async fn list_procedures(
        &self,
        pool: &ConnectionPool,
        schema: &str,
    ) -> std::result::Result<Vec<String>, ApplicationError>;

    async fn list_functions(
        &self,
        pool: &ConnectionPool,
        schema: &str,
    ) -> std::result::Result<Vec<String>, ApplicationError>;

    async fn list_sequences(
        &self,
        pool: &ConnectionPool,
        schema: &str,
    ) -> std::result::Result<Vec<String>, ApplicationError>;

    async fn list_extensions(&self, pool: &ConnectionPool) -> std::result::Result<Vec<String>, ApplicationError>;

    async fn list_columns(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        table: &str,
    ) -> std::result::Result<Vec<ColumnMeta>, ApplicationError>;

    async fn list_primary_keys(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        table: &str,
    ) -> std::result::Result<Vec<String>, ApplicationError>;

    async fn get_table_info(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        table: &str,
    ) -> std::result::Result<TableInfo, ApplicationError>;
}

/// Convenience trait combining both driver traits.
///
/// Most drivers will implement both [`DatabaseDriver`] and [`SchemaIntrospector`],
/// so this trait provides a combined interface for easier use.
#[async_trait]
pub trait FullDatabaseDriver: DatabaseDriver + SchemaIntrospector + Send + Sync {}

impl<T> FullDatabaseDriver for T where T: DatabaseDriver + SchemaIntrospector + Send + Sync {}

/// Table information returned by [`SchemaIntrospector::get_table_info`].
#[derive(Debug, Clone)]
pub struct TableInfo {
    pub columns: Vec<ColumnMeta>,
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKeyInfo>,
}

/// Foreign key information for a table.
#[derive(Debug, Clone)]
pub struct ForeignKeyInfo {
    pub constraint_name: String,
    pub columns: Vec<String>,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
}

#[cfg(test)]
mod mock_driver {
    use super::*;

    pub struct MockDatabaseDriver;

    #[async_trait]
    impl DatabaseDriver for MockDatabaseDriver {
        fn engine(&self) -> Engine {
            Engine::Postgres
        }

        fn display_name(&self) -> &'static str {
            "Mock PostgreSQL"
        }

        async fn connect(&self, _opts: &ConnectOpts) -> std::result::Result<ConnectionPool, ApplicationError> {
            Ok(ConnectionPool::new(Engine::Postgres, "mock_pool"))
        }

        async fn ping(&self, _pool: &ConnectionPool) -> std::result::Result<String, ApplicationError> {
            Ok("mock-1.0".to_string())
        }

        async fn execute_query(
            &self,
            _pool: &ConnectionPool,
            _sql: &str,
        ) -> std::result::Result<QueryResult, ApplicationError> {
            Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                rows_affected: Some(0),
                total_rows: None,
            })
        }

        async fn execute_in_schema(
            &self,
            _pool: &ConnectionPool,
            _schema: &str,
            _sql: &str,
        ) -> std::result::Result<QueryResult, ApplicationError> {
            Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                rows_affected: Some(0),
                total_rows: None,
            })
        }

        async fn execute_paginated(
            &self,
            _pool: &ConnectionPool,
            _schema: &str,
            _sql: &str,
            _limit: u64,
            _offset: u64,
        ) -> std::result::Result<QueryResult, ApplicationError> {
            Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                rows_affected: Some(0),
                total_rows: None,
            })
        }

        async fn has_rows_after(
            &self,
            _pool: &ConnectionPool,
            _schema: &str,
            _sql: &str,
            _offset: u64,
        ) -> std::result::Result<bool, ApplicationError> {
            Ok(false)
        }

        async fn count_rows(
            &self,
            _pool: &ConnectionPool,
            _schema: &str,
            _sql: &str,
        ) -> std::result::Result<Option<u64>, ApplicationError> {
            Ok(Some(0))
        }

        async fn set_search_path(
            &self,
            _pool: &ConnectionPool,
            _schema: &str,
        ) -> std::result::Result<(), ApplicationError> {
            Ok(())
        }

        async fn run_in_transaction<F>(
            &self,
            _pool: &ConnectionPool,
            _schema: &str,
            _statements: &[String],
            mut _after_each: F,
        ) -> std::result::Result<Vec<u64>, ApplicationError>
        where
            F: FnMut(usize, u64) -> std::result::Result<(), ApplicationError> + Send + Sync + 'static,
        {
            Ok(vec![])
        }
    }

    pub struct MockSchemaIntrospector;

    #[async_trait]
    impl SchemaIntrospector for MockSchemaIntrospector {
        async fn list_databases(&self, _pool: &ConnectionPool) -> std::result::Result<Vec<String>, ApplicationError> {
            Ok(vec!["test_db".to_string()])
        }

        async fn list_schemas(&self, _pool: &ConnectionPool) -> std::result::Result<Vec<String>, ApplicationError> {
            Ok(vec!["public".to_string()])
        }

        async fn list_tables(
            &self,
            _pool: &ConnectionPool,
            _schema: &str,
        ) -> std::result::Result<Vec<String>, ApplicationError> {
            Ok(vec!["users".to_string()])
        }

        async fn list_views(
            &self,
            _pool: &ConnectionPool,
            _schema: &str,
        ) -> std::result::Result<Vec<String>, ApplicationError> {
            Ok(vec![])
        }

        async fn list_matviews(
            &self,
            _pool: &ConnectionPool,
            _schema: &str,
        ) -> std::result::Result<Vec<String>, ApplicationError> {
            Ok(vec![])
        }

        async fn list_procedures(
            &self,
            _pool: &ConnectionPool,
            _schema: &str,
        ) -> std::result::Result<Vec<String>, ApplicationError> {
            Ok(vec![])
        }

        async fn list_functions(
            &self,
            _pool: &ConnectionPool,
            _schema: &str,
        ) -> std::result::Result<Vec<String>, ApplicationError> {
            Ok(vec![])
        }

        async fn list_sequences(
            &self,
            _pool: &ConnectionPool,
            _schema: &str,
        ) -> std::result::Result<Vec<String>, ApplicationError> {
            Ok(vec![])
        }

        async fn list_extensions(&self, _pool: &ConnectionPool) -> std::result::Result<Vec<String>, ApplicationError> {
            Ok(vec![])
        }

        async fn list_columns(
            &self,
            _pool: &ConnectionPool,
            _schema: &str,
            _table: &str,
        ) -> std::result::Result<Vec<ColumnMeta>, ApplicationError> {
            Ok(vec![ColumnMeta {
                name: "id".to_string(),
                type_name: "integer".to_string(),
                type_display: "integer".to_string(),
                comment: None,
            }])
        }

        async fn list_primary_keys(
            &self,
            _pool: &ConnectionPool,
            _schema: &str,
            _table: &str,
        ) -> std::result::Result<Vec<String>, ApplicationError> {
            Ok(vec!["id".to_string()])
        }

        async fn get_table_info(
            &self,
            _pool: &ConnectionPool,
            _schema: &str,
            _table: &str,
        ) -> std::result::Result<TableInfo, ApplicationError> {
            Ok(TableInfo {
                columns: vec![],
                primary_key: vec![],
                foreign_keys: vec![],
            })
        }
    }

    #[tokio::test]
    async fn mock_driver_connect_and_ping() {
        let driver = MockDatabaseDriver;
        let pool = driver.connect(&ConnectOpts::new("postgres://localhost/test")).await.unwrap();
        assert_eq!(pool.engine(), Engine::Postgres);
        let version = driver.ping(&pool).await.unwrap();
        assert_eq!(version, "mock-1.0");
    }

    #[tokio::test]
    async fn mock_introspector_list_databases() {
        let introspector = MockSchemaIntrospector;
        let pool = ConnectionPool::new(Engine::Postgres, "mock_pool");
        let dbs = introspector.list_databases(&pool).await.unwrap();
        assert_eq!(dbs, vec!["test_db"]);
    }
}
