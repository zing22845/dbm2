use rusqlite::Connection;

use crate::StoreResult;

const INIT_SCHEMA: &str = r#"
-- Schema migration tracking
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Key-value store (session, preferences, etc.)
CREATE TABLE IF NOT EXISTS kv_store (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Managed database instances discovered or registered
CREATE TABLE IF NOT EXISTS managed_instances (
    id TEXT PRIMARY KEY,
    fingerprint TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    engine TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    socket_path TEXT,
    data_dir TEXT,
    env_label TEXT,
    registered_at TEXT NOT NULL DEFAULT (datetime('now')),
    version_full TEXT,
    version_short TEXT,
    version_checked_at TEXT,
    lifecycle_status TEXT,
    lifecycle_checked_at TEXT,
    lifecycle_detail TEXT
);

-- Discovery scan records
CREATE TABLE IF NOT EXISTS discovery_scans (
    id TEXT PRIMARY KEY,
    started_at TEXT NOT NULL,
    completed_at TEXT NOT NULL,
    instance_count INTEGER NOT NULL
);

-- Cached discovery results
CREATE TABLE IF NOT EXISTS discovery_cache (
    discovery_id TEXT PRIMARY KEY,
    scan_id TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    engine TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    socket_path TEXT,
    data_dir TEXT,
    systemd_unit TEXT,
    version TEXT,
    status TEXT NOT NULL,
    sources_json TEXT NOT NULL,
    confidence TEXT NOT NULL,
    already_registered INTEGER NOT NULL,
    registered_instance_id TEXT,
    scanned_at TEXT NOT NULL,
    FOREIGN KEY (scan_id) REFERENCES discovery_scans(id)
);

CREATE INDEX IF NOT EXISTS idx_discovery_cache_scan ON discovery_cache(scan_id);
CREATE INDEX IF NOT EXISTS idx_discovery_cache_fingerprint ON discovery_cache(fingerprint);

-- Per-instance connection credentials
CREATE TABLE IF NOT EXISTS instance_connections (
    id TEXT PRIMARY KEY,
    instance_id TEXT NOT NULL
        REFERENCES managed_instances(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    username TEXT NOT NULL,
    database_name TEXT NOT NULL,
    password_nonce BLOB,
    password_enc BLOB,
    ssl_mode TEXT NOT NULL DEFAULT 'prefer',
    env_label TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    test_succeeded_at TEXT,
    test_failed_at TEXT,
    UNIQUE (instance_id, name COLLATE NOCASE)
);

CREATE INDEX IF NOT EXISTS idx_instance_connections_instance
    ON instance_connections(instance_id);

-- SQL execution history per connection
CREATE TABLE IF NOT EXISTS sql_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id TEXT NOT NULL
        REFERENCES instance_connections(id) ON DELETE CASCADE,
    sql_text TEXT NOT NULL,
    executed_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_sql_history_connection_time
    ON sql_history(connection_id, executed_at DESC);

CREATE UNIQUE INDEX IF NOT EXISTS idx_sql_history_connection_sql
    ON sql_history(connection_id, sql_text);

-- Audit log
CREATE TABLE IF NOT EXISTS audit_log (
    id TEXT PRIMARY KEY,
    action TEXT NOT NULL,
    target TEXT NOT NULL,
    detail_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_audit_log_created ON audit_log(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_log_action ON audit_log(action);
"#;

pub fn migrate(conn: &Connection) -> StoreResult<()> {
    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if current < 1 {
        conn.execute_batch(INIT_SCHEMA)?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (1)", [])?;
    }

    // Future migrations: add `if current < 2 { ... }` blocks here.

    Ok(())
}
