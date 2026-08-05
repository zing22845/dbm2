use rusqlite::{OptionalExtension, params};

use crate::{Store, StoreResult};

const KEY_TUI_SESSION: &str = "tui_session";

impl Store {
    pub fn kv_get(&self, key: &str) -> StoreResult<Option<String>> {
        let val = self
            .sqlite()
            .query_row(
                "SELECT value FROM kv_store WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(val)
    }

    pub fn kv_set(&self, key: &str, value: &str) -> StoreResult<()> {
        self.sqlite().execute(
            "INSERT INTO kv_store (key, value, updated_at)
             VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn kv_delete(&self, key: &str) -> StoreResult<bool> {
        let changes = self
            .sqlite()
            .execute("DELETE FROM kv_store WHERE key = ?1", params![key])?;
        Ok(changes > 0)
    }
}

pub fn kv_load_tui_session() -> StoreResult<Option<String>> {
    Store::open_default()?.kv_get(KEY_TUI_SESSION)
}

pub fn kv_save_tui_session(json: &str) -> StoreResult<()> {
    Store::open_default()?.kv_set(KEY_TUI_SESSION, json)
}
