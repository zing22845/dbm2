use std::collections::HashMap;

use rusqlite::params;

use crate::StoreError;
use crate::StoreResult;

/// Default max successful SQL statements kept per connection.
pub const SQL_HISTORY_MAX_PER_CONNECTION: usize = 100;

impl super::Store {
    /// Load all history grouped by `(instance_name, connection_name)`, newest first per group.
    pub fn load_sql_history(&self) -> StoreResult<HashMap<(String, String), Vec<String>>> {
        let mut stmt = self.sqlite().prepare(
            "SELECT mi.name, ic.name, sh.sql_text
             FROM sql_history sh
             INNER JOIN instance_connections ic ON ic.id = sh.connection_id
             INNER JOIN managed_instances mi ON mi.id = ic.instance_id
             ORDER BY mi.name ASC, ic.name ASC, sh.id DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        let mut map: HashMap<(String, String), Vec<String>> = HashMap::new();
        for row in rows {
            let (instance, connection, sql) = row?;
            map.entry((instance, connection)).or_default().push(sql);
        }
        Ok(map)
    }

    /// Record one successful SQL for a connection; no-op if trimmed SQL empty or connection missing.
    pub fn record_sql_history(
        &self,
        instance_name: &str,
        connection_name: &str,
        sql: &str,
    ) -> StoreResult<()> {
        let trimmed = sql.trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        let connection = match self.get_instance_connection(instance_name, connection_name) {
            Ok(conn) => conn,
            Err(StoreError::NotFound(_)) => return Ok(()),
            Err(err) => return Err(err),
        };

        let db = self.sqlite();
        db.execute(
            "DELETE FROM sql_history WHERE connection_id = ?1 AND sql_text = ?2",
            params![connection.id, trimmed],
        )?;
        db.execute(
            "INSERT INTO sql_history (connection_id, sql_text) VALUES (?1, ?2)",
            params![connection.id, trimmed],
        )?;
        db.execute(
            "DELETE FROM sql_history
             WHERE connection_id = ?1
               AND id NOT IN (
                 SELECT id FROM sql_history
                 WHERE connection_id = ?1
                 ORDER BY id DESC
                 LIMIT ?2
               )",
            params![
                connection.id,
                i64::try_from(SQL_HISTORY_MAX_PER_CONNECTION).unwrap_or(100)
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    fn seed_instance_and_connection(store: &Store) -> (String, String) {
        store
            .sqlite()
            .execute_batch(
                "INSERT INTO managed_instances (id, fingerprint, name, engine, host, port, registered_at)
                 VALUES ('inst1', 'fp1', 'pg-local', 'postgres', '127.0.0.1', 5432, datetime('now'));
                 INSERT INTO instance_connections (
                    id, instance_id, name, username, database_name, ssl_mode, created_at, updated_at
                 ) VALUES (
                    'conn1', 'inst1', 'default', 'postgres', 'postgres', 'prefer', datetime('now'), datetime('now')
                 );",
            )
            .unwrap();
        ("pg-local".into(), "default".into())
    }

    #[test]
    fn record_dedupes_and_orders_newest_first() {
        let store = Store::open_in_memory().unwrap();
        let (instance, connection) = seed_instance_and_connection(&store);

        store
            .record_sql_history(&instance, &connection, "SELECT 1")
            .unwrap();
        store
            .record_sql_history(&instance, &connection, "SELECT 2")
            .unwrap();
        store
            .record_sql_history(&instance, &connection, "SELECT 1")
            .unwrap();

        let map = store.load_sql_history().unwrap();
        let entries = map.get(&(instance, connection)).unwrap();
        assert_eq!(entries.as_slice(), &["SELECT 1".to_string(), "SELECT 2".to_string()]);
    }

    #[test]
    fn truncates_to_max_per_connection() {
        let store = Store::open_in_memory().unwrap();
        let (instance, connection) = seed_instance_and_connection(&store);

        for i in 0..SQL_HISTORY_MAX_PER_CONNECTION + 5 {
            store
                .record_sql_history(&instance, &connection, &format!("SELECT {i}"))
                .unwrap();
        }

        let map = store.load_sql_history().unwrap();
        let entries = map.get(&(instance, connection)).unwrap();
        assert_eq!(entries.len(), SQL_HISTORY_MAX_PER_CONNECTION);
        assert_eq!(entries[0], format!("SELECT {}", SQL_HISTORY_MAX_PER_CONNECTION + 4));
    }
}
