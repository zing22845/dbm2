use rusqlite::params;
use serde_json::Value;

use crate::StoreResult;

pub(super) fn record_audit(
    conn: &rusqlite::Connection,
    action: &str,
    target: &str,
    detail: Value,
) -> StoreResult<()> {
    let id = format!("aud_{}", uuid::Uuid::new_v4().simple());
    let detail_json = serde_json::to_string(&detail).unwrap_or_else(|_| "{}".into());
    conn.execute(
        "INSERT INTO audit_log (id, action, target, detail_json)
         VALUES (?1, ?2, ?3, ?4)",
        params![id, action, target, detail_json],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn record_audit_persists_row() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE audit_log (
                id TEXT PRIMARY KEY,
                action TEXT NOT NULL,
                target TEXT NOT NULL,
                detail_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .unwrap();
        record_audit(
            &conn,
            "instance_connections.delete",
            "pg:admin",
            json!({ "instance": "pg", "connection": "admin" }),
        )
        .unwrap();
        let action: String = conn
            .query_row("SELECT action FROM audit_log", [], |row| row.get(0))
            .unwrap();
        assert_eq!(action, "instance_connections.delete");
    }
}
