use rusqlite::Connection;

use crate::StoreResult;

const MIGRATION_1: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS connections (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    engine TEXT NOT NULL DEFAULT 'postgres',
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    username TEXT NOT NULL,
    database_name TEXT NOT NULL,
    env_label TEXT,
    password_nonce BLOB,
    password_enc BLOB,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_connections_default ON connections(is_default);
"#;

const MIGRATION_2: &str = r#"
CREATE TABLE IF NOT EXISTS discovery_scans (
    id TEXT PRIMARY KEY,
    started_at TEXT NOT NULL,
    completed_at TEXT NOT NULL,
    instance_count INTEGER NOT NULL
);

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
    registered_at TEXT NOT NULL DEFAULT (datetime('now'))
);
"#;

const MIGRATION_3: &str = r#"
CREATE TABLE IF NOT EXISTS instance_connections (
    id TEXT PRIMARY KEY,
    instance_id TEXT NOT NULL REFERENCES managed_instances(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    username TEXT NOT NULL,
    database_name TEXT NOT NULL,
    password_nonce BLOB,
    password_enc BLOB,
    ssl_mode TEXT NOT NULL DEFAULT 'prefer',
    is_default INTEGER NOT NULL DEFAULT 0,
    env_label TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (instance_id, name COLLATE NOCASE)
);

CREATE INDEX IF NOT EXISTS idx_instance_connections_instance ON instance_connections(instance_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_instance_connections_default
    ON instance_connections(instance_id) WHERE is_default = 1;

DROP TABLE IF EXISTS connections;
"#;

const MIGRATION_4: &str = r#"
DROP INDEX IF EXISTS idx_instance_connections_default;
ALTER TABLE instance_connections DROP COLUMN is_default;
"#;

const MIGRATION_5: &str = r#"
CREATE TABLE IF NOT EXISTS tui_session (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    version INTEGER NOT NULL,
    snapshot_json TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
"#;

const MIGRATION_6: &str = r#"
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
"#;

const MIGRATION_7: &str = r#"
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

const MIGRATION_8: &str = r#"
ALTER TABLE managed_instances ADD COLUMN version_full TEXT;
ALTER TABLE managed_instances ADD COLUMN version_short TEXT;
ALTER TABLE managed_instances ADD COLUMN version_checked_at TEXT;
"#;

const MIGRATION_9: &str = r#"
ALTER TABLE managed_instances ADD COLUMN lifecycle_status TEXT;
ALTER TABLE managed_instances ADD COLUMN lifecycle_checked_at TEXT;
ALTER TABLE managed_instances ADD COLUMN lifecycle_detail TEXT;
"#;

const MIGRATION_10: &str = r#"
CREATE TABLE IF NOT EXISTS kv_store (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
INSERT INTO kv_store (key, value, updated_at)
SELECT 'tui_session', snapshot_json, updated_at
FROM tui_session
WHERE id = 1
ON CONFLICT(key) DO NOTHING;
"#;

const MIGRATION_11: &str = r#"
ALTER TABLE instance_connections ADD COLUMN test_succeeded_at TEXT;
ALTER TABLE instance_connections ADD COLUMN test_failed_at TEXT;
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
        conn.execute_batch(MIGRATION_1)?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (1)", [])?;
    }

    if current < 2 {
        conn.execute_batch(MIGRATION_2)?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (2)", [])?;
    }

    if current < 3 {
        conn.execute_batch(MIGRATION_3)?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (3)", [])?;
    }

    if current < 4 {
        conn.execute_batch(MIGRATION_4)?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (4)", [])?;
    }

    if current < 5 {
        conn.execute_batch(MIGRATION_5)?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (5)", [])?;
    }

    if current < 6 {
        conn.execute_batch(MIGRATION_6)?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (6)", [])?;
    }

    if current < 7 {
        conn.execute_batch(MIGRATION_7)?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (7)", [])?;
    }

    if current < 8 {
        conn.execute_batch(MIGRATION_8)?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (8)", [])?;
    }

    if current < 9 {
        conn.execute_batch(MIGRATION_9)?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (9)", [])?;
    }

    if current < 10 {
        conn.execute_batch(MIGRATION_10)?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (10)", [])?;
    }

    if current < 11 {
        conn.execute_batch(MIGRATION_11)?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (11)", [])?;
    }

    Ok(())
}
