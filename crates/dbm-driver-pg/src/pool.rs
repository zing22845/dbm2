use dbm_core::{ConnectOpts, DbmError, QueryResult, Result};
use deadpool_postgres::{
    BuildError, GenericClient, Manager, ManagerConfig, Pool, PoolError, RecyclingMethod,
};

use crate::format_postgres_error;
use crate::ident::quote_ident;

pub struct PostgresPool {
    pool: Pool,
    url: String,
}

impl Clone for PostgresPool {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            url: self.url.clone(),
        }
    }
}

impl PostgresPool {
    pub fn new(opts: &ConnectOpts) -> Result<Self> {
        let parsed = opts.parse()?;
        // tokio-postgres rejects `verify-full`-style sslmode values, so map the
        // URL parameter ourselves and hand the mode to the config explicitly.
        let (connect_url, ssl_mode) = crate::tls::split_ssl_mode(&parsed.url)?;
        let mut pg_config = connect_url
            .parse::<tokio_postgres::Config>()
            .map_err(|e| DbmError::InvalidUrl(e.to_string()))?;
        pg_config.ssl_mode(ssl_mode.tokio_ssl_mode());

        let manager = Manager::from_config(
            pg_config,
            crate::tls::connector(ssl_mode)?,
            ManagerConfig {
                recycling_method: RecyclingMethod::Fast,
            },
        );

        // `build()` returns `BuildError`, not `PoolError` — do not use `format_pool_get_error` here.
        let pool = Pool::builder(manager)
            .max_size(8)
            .build()
            .map_err(map_build_error(&parsed.url))?;

        Ok(Self {
            pool,
            url: parsed.url,
        })
    }

    pub async fn get(&self) -> Result<deadpool_postgres::Object> {
        self.pool
            .get()
            .await
            .map_err(|e| format_pool_get_error(&self.url, e))
    }

    pub async fn ping(&self) -> Result<String> {
        let client = self.get().await?;
        let row = client
            .query_one("SELECT version()", &[])
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        Ok(row.get::<_, String>(0))
    }

    pub async fn execute(&self, sql: &str) -> Result<QueryResult> {
        let client = self.get().await?;
        crate::client::execute_on_client(&client, sql).await
    }

    pub async fn execute_in_schema(&self, schema: &str, sql: &str) -> Result<QueryResult> {
        let mut client = self.get().await?;
        crate::client::execute_in_schema_on_client(&mut client, schema, sql).await
    }

    pub async fn execute_paginated_in_schema(
        &self,
        schema: &str,
        sql: &str,
        limit: u64,
        offset: u64,
    ) -> Result<QueryResult> {
        let mut client = self.get().await?;
        crate::client::execute_paginated_in_schema_on_client(
            &mut client,
            schema,
            sql,
            limit,
            offset,
        )
        .await
    }

    pub async fn has_paginated_rows_in_schema(
        &self,
        schema: &str,
        sql: &str,
        offset: u64,
    ) -> Result<bool> {
        let mut client = self.get().await?;
        crate::client::has_paginated_rows_in_schema_on_client(&mut client, schema, sql, offset)
            .await
    }

    pub async fn count_in_schema(&self, schema: &str, sql: &str) -> Result<Option<u64>> {
        let mut client = self.get().await?;
        crate::client::count_in_schema_on_client(&mut client, schema, sql).await
    }

    pub async fn set_search_path(&self, schema: &str) -> Result<()> {
        let client = self.get().await?;
        let sql = format!("SET search_path TO {}, public", quote_ident(schema));
        client
            .batch_execute(&sql)
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        Ok(())
    }

    pub async fn list_databases(&self) -> Result<Vec<String>> {
        let client = self.get().await?;
        let rows = client
            .query(
                "SELECT datname FROM pg_database \
                 WHERE datistemplate = false \
                 ORDER BY datname",
                &[],
            )
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        Ok(rows.iter().map(|row| row.get::<_, String>(0)).collect())
    }

    pub async fn list_schemas(&self) -> Result<Vec<String>> {
        let client = self.get().await?;
        let rows = client
            .query(
                "SELECT schema_name FROM information_schema.schemata \
                 WHERE schema_name NOT IN ('pg_catalog', 'information_schema') \
                   AND schema_name NOT LIKE 'pg_toast%' \
                   AND schema_name NOT LIKE 'pg_temp_%' \
                 ORDER BY schema_name",
                &[],
            )
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        Ok(rows.iter().map(|row| row.get::<_, String>(0)).collect())
    }

    pub async fn list_tables(&self, schema: &str) -> Result<Vec<String>> {
        let client = self.get().await?;
        let rows = client
            .query(
                "SELECT table_name FROM information_schema.tables \
                 WHERE table_schema = $1 AND table_type = 'BASE TABLE' \
                 ORDER BY table_name",
                &[&schema],
            )
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;

        Ok(rows.iter().map(|row| row.get::<_, String>(0)).collect())
    }

    pub async fn list_views(&self, schema: &str) -> Result<Vec<String>> {
        let client = self.get().await?;
        let rows = client
            .query(
                "SELECT table_name FROM information_schema.views \
                 WHERE table_schema = $1 \
                 ORDER BY table_name",
                &[&schema],
            )
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        Ok(rows.iter().map(|row| row.get::<_, String>(0)).collect())
    }

    pub async fn list_matviews(&self, schema: &str) -> Result<Vec<String>> {
        let client = self.get().await?;
        let rows = client
            .query(
                "SELECT matviewname FROM pg_matviews \
                 WHERE schemaname = $1 \
                 ORDER BY matviewname",
                &[&schema],
            )
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        Ok(rows.iter().map(|row| row.get::<_, String>(0)).collect())
    }

    pub async fn list_procedures(&self, schema: &str) -> Result<Vec<String>> {
        let client = self.get().await?;
        let rows = client
            .query(
                "SELECT p.proname::text \
                 FROM pg_proc p \
                 JOIN pg_namespace n ON n.oid = p.pronamespace \
                 WHERE n.nspname = $1 AND p.prokind = 'p' \
                 ORDER BY p.proname",
                &[&schema],
            )
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        Ok(rows.iter().map(|row| row.get::<_, String>(0)).collect())
    }

    pub async fn list_functions(&self, schema: &str) -> Result<Vec<String>> {
        let client = self.get().await?;
        let rows = client
            .query(
                "SELECT p.proname::text \
                 FROM pg_proc p \
                 JOIN pg_namespace n ON n.oid = p.pronamespace \
                 WHERE n.nspname = $1 AND p.prokind = 'f' \
                 ORDER BY p.proname",
                &[&schema],
            )
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        Ok(rows.iter().map(|row| row.get::<_, String>(0)).collect())
    }

    pub async fn list_sequences(&self, schema: &str) -> Result<Vec<String>> {
        let client = self.get().await?;
        let rows = client
            .query(
                "SELECT sequence_name FROM information_schema.sequences \
                 WHERE sequence_schema = $1 \
                 ORDER BY sequence_name",
                &[&schema],
            )
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        Ok(rows.iter().map(|row| row.get::<_, String>(0)).collect())
    }

    pub async fn list_extensions(&self) -> Result<Vec<String>> {
        let client = self.get().await?;
        let rows = client
            .query("SELECT extname FROM pg_extension ORDER BY extname", &[])
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        Ok(rows.iter().map(|row| row.get::<_, String>(0)).collect())
    }

    pub async fn list_columns(
        &self,
        schema: &str,
        table: &str,
    ) -> Result<Vec<dbm_core::ColumnMeta>> {
        let client = self.get().await?;
        let rows = client
            .query(
                "SELECT column_name, data_type, \
                        CASE \
                            WHEN character_maximum_length IS NOT NULL \
                                THEN data_type || '(' || character_maximum_length::text || ')' \
                            WHEN numeric_precision IS NOT NULL AND numeric_scale IS NOT NULL \
                                THEN data_type || '(' || numeric_precision::text || ',' || numeric_scale::text || ')' \
                            WHEN numeric_precision IS NOT NULL \
                                THEN data_type || '(' || numeric_precision::text || ')' \
                            ELSE data_type \
                        END AS type_display, \
                        pg_catalog.col_description( \
                            (quote_ident(table_schema) || '.' || quote_ident(table_name))::regclass, \
                            ordinal_position::int \
                        ) AS column_comment \
                 FROM information_schema.columns \
                 WHERE table_schema = $1 AND table_name = $2 \
                 ORDER BY ordinal_position",
                &[&schema, &table],
            )
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;

        Ok(rows
            .iter()
            .map(|row| dbm_core::ColumnMeta {
                name: row.get(0),
                type_name: row.get(1),
                type_display: row.get(2),
                comment: row.get::<_, Option<String>>(3),
            })
            .collect())
    }

    pub async fn list_primary_keys(&self, schema: &str, table: &str) -> Result<Vec<String>> {
        let client = self.get().await?;
        let rows = client
            .query(PRIMARY_KEYS_SQL, &[&schema, &table])
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        Ok(rows.iter().map(|row| row.get::<_, String>(0)).collect())
    }

    /// Run statements in one transaction with `SET LOCAL search_path`.
    /// `after_each(index, rows_affected)` may return `Err` to abort (rollback).
    pub async fn execute_statements_in_transaction<F>(
        &self,
        schema: &str,
        statements: &[String],
        mut after_each: F,
    ) -> Result<Vec<u64>>
    where
        F: FnMut(usize, u64) -> Result<()>,
    {
        let mut client = self.get().await?;
        let txn = client
            .transaction()
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        let set_path = format!("SET LOCAL search_path TO {}, public", quote_ident(schema));
        txn.batch_execute(&set_path)
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;

        let mut affected = Vec::with_capacity(statements.len());
        for (index, sql) in statements.iter().enumerate() {
            let trimmed = sql.trim();
            if trimmed.is_empty() {
                continue;
            }
            let rows = txn
                .execute(trimmed, &[])
                .await
                .map_err(|e| crate::dbm_error_from_postgres(&e))?;
            after_each(index, rows)?;
            affected.push(rows);
        }

        txn.commit()
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        Ok(affected)
    }
}

/// Parameterized SQL for `list_primary_keys` (`$1` = schema, `$2` = table).
pub const PRIMARY_KEYS_SQL: &str = "\
SELECT a.attname \
FROM pg_index i \
JOIN pg_class c ON c.oid = i.indrelid \
JOIN pg_namespace n ON n.oid = c.relnamespace \
JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY (i.indkey) \
WHERE i.indisprimary \
  AND n.nspname = $1 \
  AND c.relname = $2 \
ORDER BY array_position(i.indkey, a.attnum)";

fn map_build_error(url: &str) -> impl FnOnce(BuildError) -> DbmError {
    let url = url.to_string();
    move |err| DbmError::database(humanize_connect_error(&url, err.to_string()))
}

fn format_pool_get_error(url: &str, err: PoolError) -> DbmError {
    match err {
        PoolError::Backend(pg_err) => {
            let formatted = format_postgres_error(&pg_err);
            DbmError::database_with_severity(
                humanize_connect_error(url, formatted.message),
                formatted.severity,
            )
        }
        other => DbmError::database(humanize_connect_error(url, other.to_string())),
    }
}

fn humanize_connect_error(url: &str, err: String) -> String {
    if err.contains("invalid configuration") && !url_has_password(url) {
        return format!(
            "{err} (password required; run `dbm instance connection add --password ...` or pass `--url`)"
        );
    }
    err
}

fn url_has_password(raw: &str) -> bool {
    let Some(auth) = raw.split("://").nth(1) else {
        return false;
    };
    let auth = auth.split('@').next().unwrap_or("");
    auth.contains(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_keys_sql_targets_pg_catalog() {
        assert!(PRIMARY_KEYS_SQL.contains("indisprimary"));
        assert!(PRIMARY_KEYS_SQL.contains("$1"));
        assert!(PRIMARY_KEYS_SQL.contains("$2"));
    }
}
