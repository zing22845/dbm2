use dbm_core::{QueryResult, Result};

use crate::ident::quote_ident;
use deadpool_postgres::GenericClient;

use crate::convert;
use crate::pagination::{self, is_read_only};

pub struct PostgresClient;

impl PostgresClient {
    pub async fn ping(pool: &crate::PostgresPool) -> Result<String> {
        pool.ping().await
    }
}

pub async fn execute_on_client<C>(client: &C, sql: &str) -> Result<QueryResult>
where
    C: GenericClient,
{
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Ok(QueryResult::empty_message("(empty query)"));
    }

    if is_read_only(trimmed) {
        let rows = client
            .query(trimmed, &[])
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        Ok(convert::rows_to_result(&rows))
    } else {
        let affected = client
            .execute(trimmed, &[])
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: Some(affected),
            total_rows: None,
        })
    }
}

pub async fn execute_in_schema_on_client<C>(client: &mut C, schema: &str, sql: &str) -> Result<QueryResult>
where
    C: GenericClient,
{
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Ok(QueryResult::empty_message("(empty query)"));
    }

    let txn = client
        .transaction()
        .await
        .map_err(|e| crate::dbm_error_from_postgres(&e))?;

    let set_path = format!(
        "SET LOCAL search_path TO {}, public",
        quote_ident(schema)
    );
    txn.batch_execute(&set_path)
        .await
        .map_err(|e| crate::dbm_error_from_postgres(&e))?;

    let result = if is_read_only(trimmed) {
        let rows = txn
            .query(trimmed, &[])
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        convert::rows_to_result(&rows)
    } else {
        let affected = txn
            .execute(trimmed, &[])
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: Some(affected),
            total_rows: None,
        }
    };

    txn.commit()
        .await
        .map_err(|e| crate::dbm_error_from_postgres(&e))?;

    Ok(result)
}

pub async fn execute_paginated_in_schema_on_client<C>(
    client: &mut C,
    schema: &str,
    sql: &str,
    limit: u64,
    offset: u64,
) -> Result<QueryResult>
where
    C: GenericClient,
{
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Ok(QueryResult::empty_message("(empty query)"));
    }

    let Some(page_sql) = pagination::paginated_select_sql(trimmed, limit, offset) else {
        return execute_in_schema_on_client(client, schema, sql).await;
    };

    let txn = client
        .transaction()
        .await
        .map_err(|e| crate::dbm_error_from_postgres(&e))?;

    let set_path = format!(
        "SET LOCAL search_path TO {}, public",
        quote_ident(schema)
    );
    txn.batch_execute(&set_path)
        .await
        .map_err(|e| crate::dbm_error_from_postgres(&e))?;

    let rows = txn
        .query(&page_sql, &[])
        .await
        .map_err(|e| crate::dbm_error_from_postgres(&e))?;
    let mut result = convert::rows_to_result(&rows);

    // Total row count over the user query (for the "Total N rows" / "page x/N"
    // toolbar). Runs inside the same transaction; `None` when the query is not
    // a single count-able SELECT.
    if let Some(count_sql) = pagination::count_select_sql(trimmed) {
        let count_row = txn
            .query_one(&count_sql, &[])
            .await
            .map_err(|e| crate::dbm_error_from_postgres(&e))?;
        let total: i64 = count_row.get(0);
        result.total_rows = Some(total.max(0) as u64);
    }

    txn.commit()
        .await
        .map_err(|e| crate::dbm_error_from_postgres(&e))?;

    Ok(result)
}

/// Whether at least one row exists at `offset` in the paginated user query.
pub async fn has_paginated_rows_in_schema_on_client<C>(
    client: &mut C,
    schema: &str,
    sql: &str,
    offset: u64,
) -> Result<bool>
where
    C: GenericClient,
{
    let result =
        execute_paginated_in_schema_on_client(client, schema, sql, 1, offset).await?;
    Ok(!result.rows.is_empty())
}

pub async fn count_in_schema_on_client<C>(client: &mut C, schema: &str, sql: &str) -> Result<Option<u64>>
where
    C: GenericClient,
{
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let Some(count_sql) = pagination::count_select_sql(trimmed) else {
        return Ok(None);
    };

    let txn = client
        .transaction()
        .await
        .map_err(|e| crate::dbm_error_from_postgres(&e))?;

    let set_path = format!(
        "SET LOCAL search_path TO {}, public",
        quote_ident(schema)
    );
    txn.batch_execute(&set_path)
        .await
        .map_err(|e| crate::dbm_error_from_postgres(&e))?;

    let count_row = txn
        .query_one(&count_sql, &[])
        .await
        .map_err(|e| crate::dbm_error_from_postgres(&e))?;
    let total: i64 = count_row.get(0);

    txn.commit()
        .await
        .map_err(|e| crate::dbm_error_from_postgres(&e))?;

    Ok(Some(total.max(0) as u64))
}
