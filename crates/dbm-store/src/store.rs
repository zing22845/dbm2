use std::path::Path;

use rusqlite::Connection;

use crate::crypto::SecretBox;
use crate::migrate::migrate;
use crate::paths::{db_path, ensure_data_dir, ensure_parent};
use crate::{StoreError, StoreResult};

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open_default() -> StoreResult<Self> {
        ensure_data_dir()?;
        let path = db_path()?;
        Self::open(&path)
    }

    pub fn open(path: &Path) -> StoreResult<Self> {
        ensure_parent(path)?;
        let conn = Connection::open(path)?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        conn.pragma_update(None, "journal_mode", "wal")?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> StoreResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub(super) fn sqlite(&self) -> &Connection {
        &self.conn
    }
}

pub(super) type EncryptedPassword = (Option<Vec<u8>>, Option<Vec<u8>>);

pub(super) fn encrypt_password(password: Option<&str>) -> StoreResult<EncryptedPassword> {
    let Some(password) = password.filter(|p| !p.is_empty()) else {
        return Ok((None, None));
    };
    let secret = SecretBox::encrypt(password)?;
    Ok((
        Some(secret.nonce().to_vec()),
        Some(secret.ciphertext().to_vec()),
    ))
}

pub fn build_database_url_from_parts(
    host: &str,
    port: u16,
    username: &str,
    database: &str,
    password: Option<&str>,
) -> StoreResult<String> {
    use url::Url;

    let mut url = Url::parse(&format!("postgresql://{username}@{host}:{port}/{database}"))
        .map_err(|e| StoreError::Other(e.to_string()))?;

    if let Some(password) = password {
        url.set_password(Some(password))
            .map_err(|_| StoreError::Other("failed to set password in URL".into()))?;
    }

    Ok(url.to_string())
}
