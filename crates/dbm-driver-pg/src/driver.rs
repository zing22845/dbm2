use async_trait::async_trait;

use dbm_core::{
    ApplicationError, ColumnMeta, ConnectOpts, ConnectionPool, DatabaseDriver, DbmError, Engine,
    QueryResult, SchemaIntrospector, TableInfo,
};

use crate::pool::PostgresPool;

pub struct PostgresDriver;

impl PostgresDriver {
    fn downcast_pool<'a>(&self, pool: &'a ConnectionPool) -> &'a PostgresPool {
        pool.downcast_ref::<PostgresPool>()
            .expect("ConnectionPool was not created by PostgresDriver")
    }
}

#[async_trait]
impl DatabaseDriver for PostgresDriver {
    fn engine(&self) -> Engine {
        Engine::Postgres
    }

    fn display_name(&self) -> &'static str {
        "PostgreSQL"
    }

    async fn connect(
        &self,
        opts: &ConnectOpts,
    ) -> std::result::Result<ConnectionPool, ApplicationError> {
        let inner = PostgresPool::new(opts).map_err(ApplicationError::from)?;
        Ok(ConnectionPool::new(Engine::Postgres, inner))
    }

    async fn ping(&self, pool: &ConnectionPool) -> std::result::Result<String, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool.ping().await.map_err(ApplicationError::from)
    }

    async fn execute_query(
        &self,
        pool: &ConnectionPool,
        sql: &str,
    ) -> std::result::Result<QueryResult, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool.execute(sql).await.map_err(ApplicationError::from)
    }

    async fn execute_in_schema(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        sql: &str,
    ) -> std::result::Result<QueryResult, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool
            .execute_in_schema(schema, sql)
            .await
            .map_err(ApplicationError::from)
    }

    async fn execute_paginated(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        sql: &str,
        limit: u64,
        offset: u64,
    ) -> std::result::Result<QueryResult, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool
            .execute_paginated_in_schema(schema, sql, limit, offset)
            .await
            .map_err(ApplicationError::from)
    }

    async fn has_rows_after(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        sql: &str,
        offset: u64,
    ) -> std::result::Result<bool, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool
            .has_paginated_rows_in_schema(schema, sql, offset)
            .await
            .map_err(ApplicationError::from)
    }

    async fn count_rows(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        sql: &str,
    ) -> std::result::Result<Option<u64>, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool
            .count_in_schema(schema, sql)
            .await
            .map_err(ApplicationError::from)
    }

    async fn set_search_path(
        &self,
        pool: &ConnectionPool,
        schema: &str,
    ) -> std::result::Result<(), ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool
            .set_search_path(schema)
            .await
            .map_err(ApplicationError::from)
    }

    async fn run_in_transaction<F>(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        statements: &[String],
        mut after_each: F,
    ) -> std::result::Result<Vec<u64>, ApplicationError>
    where
        F: FnMut(usize, u64) -> std::result::Result<(), ApplicationError> + Send + Sync + 'static,
    {
        let pg_pool = self.downcast_pool(pool);
        let adapted_after_each = move |idx, cnt| after_each(idx, cnt).map_err(DbmError::from);
        pg_pool
            .execute_statements_in_transaction(schema, statements, adapted_after_each)
            .await
            .map_err(ApplicationError::from)
    }
}

#[async_trait]
impl SchemaIntrospector for PostgresDriver {
    async fn list_databases(
        &self,
        pool: &ConnectionPool,
    ) -> std::result::Result<Vec<String>, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool
            .list_databases()
            .await
            .map_err(ApplicationError::from)
    }

    async fn list_schemas(
        &self,
        pool: &ConnectionPool,
    ) -> std::result::Result<Vec<String>, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool.list_schemas().await.map_err(ApplicationError::from)
    }

    async fn list_tables(
        &self,
        pool: &ConnectionPool,
        schema: &str,
    ) -> std::result::Result<Vec<String>, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool
            .list_tables(schema)
            .await
            .map_err(ApplicationError::from)
    }

    async fn list_views(
        &self,
        pool: &ConnectionPool,
        schema: &str,
    ) -> std::result::Result<Vec<String>, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool
            .list_views(schema)
            .await
            .map_err(ApplicationError::from)
    }

    async fn list_matviews(
        &self,
        pool: &ConnectionPool,
        schema: &str,
    ) -> std::result::Result<Vec<String>, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool
            .list_matviews(schema)
            .await
            .map_err(ApplicationError::from)
    }

    async fn list_procedures(
        &self,
        pool: &ConnectionPool,
        schema: &str,
    ) -> std::result::Result<Vec<String>, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool
            .list_procedures(schema)
            .await
            .map_err(ApplicationError::from)
    }

    async fn list_functions(
        &self,
        pool: &ConnectionPool,
        schema: &str,
    ) -> std::result::Result<Vec<String>, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool
            .list_functions(schema)
            .await
            .map_err(ApplicationError::from)
    }

    async fn list_sequences(
        &self,
        pool: &ConnectionPool,
        schema: &str,
    ) -> std::result::Result<Vec<String>, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool
            .list_sequences(schema)
            .await
            .map_err(ApplicationError::from)
    }

    async fn list_extensions(
        &self,
        pool: &ConnectionPool,
    ) -> std::result::Result<Vec<String>, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool
            .list_extensions()
            .await
            .map_err(ApplicationError::from)
    }

    async fn list_columns(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        table: &str,
    ) -> std::result::Result<Vec<ColumnMeta>, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool
            .list_columns(schema, table)
            .await
            .map_err(ApplicationError::from)
    }

    async fn list_primary_keys(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        table: &str,
    ) -> std::result::Result<Vec<String>, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        pg_pool
            .list_primary_keys(schema, table)
            .await
            .map_err(ApplicationError::from)
    }

    async fn get_table_info(
        &self,
        pool: &ConnectionPool,
        schema: &str,
        table: &str,
    ) -> std::result::Result<TableInfo, ApplicationError> {
        let pg_pool = self.downcast_pool(pool);
        let columns = pg_pool
            .list_columns(schema, table)
            .await
            .map_err(ApplicationError::from)?;
        let primary_key = pg_pool
            .list_primary_keys(schema, table)
            .await
            .map_err(ApplicationError::from)?;
        Ok(TableInfo {
            columns,
            primary_key,
            foreign_keys: vec![],
        })
    }
}
