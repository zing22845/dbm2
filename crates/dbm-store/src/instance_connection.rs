use rusqlite::{OptionalExtension, params};
use serde_json::json;

use crate::audit::record_audit;
use crate::crypto::SecretBox;
use crate::instance::ManagedInstance;
use crate::register_precheck::{PrecheckIssue, PrecheckLevel};
use crate::store::{build_database_url_from_parts, encrypt_password};
use crate::{StoreError, StoreResult};

#[derive(Debug, Clone, PartialEq)]
pub struct InstanceConnection {
    pub id: String,
    pub instance_id: String,
    pub name: String,
    pub username: String,
    pub database: String,
    pub has_password: bool,
    pub ssl_mode: String,
    pub env_label: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Timestamp of the most recent successful connection test, if any.
    pub test_succeeded_at: Option<String>,
    /// Timestamp of the most recent failed connection test, if any.
    pub test_failed_at: Option<String>,
}

impl InstanceConnection {
    pub fn display_target(&self, instance: &ManagedInstance) -> String {
        format!(
            "{}@{}:{}/{}",
            self.username, instance.host, instance.port, self.database
        )
    }
}

#[derive(Debug, Clone)]
pub struct NewInstanceConnection {
    pub name: String,
    pub username: String,
    pub database: String,
    pub password: Option<String>,
    pub ssl_mode: Option<String>,
    pub env_label: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpdateInstanceConnection {
    pub name: Option<String>,
    pub username: Option<String>,
    pub database: Option<String>,
    /// `None` = leave password unchanged; `Some(None)` = clear; `Some(Some(p))` = set.
    pub password: Option<Option<String>>,
    pub ssl_mode: Option<String>,
    pub env_label: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConnectionPrecheck {
    pub ok: bool,
    pub version: Option<String>,
    pub issues: Vec<PrecheckIssue>,
}

impl super::Store {
    pub fn list_instance_connections(
        &self,
        instance_name: &str,
    ) -> StoreResult<Vec<InstanceConnection>> {
        let instance = self.get_managed_instance_by_name(instance_name)?;
        let mut stmt = self.sqlite().prepare(
            "SELECT id, instance_id, name, username, database_name, password_enc IS NOT NULL,
                    ssl_mode, env_label, created_at, updated_at, test_succeeded_at, test_failed_at
             FROM instance_connections
             WHERE instance_id = ?1
             ORDER BY name ASC",
        )?;
        let rows = stmt.query_map(params![instance.id], map_instance_connection_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_instance_connection(
        &self,
        instance_name: &str,
        connection_name: &str,
    ) -> StoreResult<InstanceConnection> {
        let instance = self.get_managed_instance_by_name(instance_name)?;
        self.get_instance_connection_for_instance(&instance, connection_name)
    }

    fn get_instance_connection_for_instance(
        &self,
        instance: &ManagedInstance,
        connection_name: &str,
    ) -> StoreResult<InstanceConnection> {
        self.sqlite()
            .query_row(
                "SELECT id, instance_id, name, username, database_name, password_enc IS NOT NULL,
                        ssl_mode, env_label, created_at, updated_at, test_succeeded_at, test_failed_at
                 FROM instance_connections
                 WHERE instance_id = ?1 AND name = ?2 COLLATE NOCASE",
                params![instance.id, connection_name],
                map_instance_connection_row,
            )
            .optional()?
            .ok_or_else(|| {
                StoreError::NotFound(format!(
                    "connection `{connection_name}` on instance `{}`",
                    instance.name
                ))
            })
    }

    fn get_instance_connection_by_id(
        &self,
        instance: &ManagedInstance,
        connection_id: &str,
    ) -> StoreResult<InstanceConnection> {
        self.sqlite()
            .query_row(
                "SELECT id, instance_id, name, username, database_name, password_enc IS NOT NULL,
                        ssl_mode, env_label, created_at, updated_at, test_succeeded_at, test_failed_at
                 FROM instance_connections
                 WHERE instance_id = ?1 AND id = ?2",
                params![instance.id, connection_id],
                map_instance_connection_row,
            )
            .optional()?
            .ok_or_else(|| {
                StoreError::NotFound(format!(
                    "connection id `{connection_id}` on instance `{}`",
                    instance.name
                ))
            })
    }

    pub fn find_first_session(&self) -> StoreResult<Option<(ManagedInstance, InstanceConnection)>> {
        let instances = self.list_managed_instances()?;
        for instance in instances {
            let conns = self.list_instance_connections(&instance.name)?;
            if let Some(conn) = conns.into_iter().next() {
                return Ok(Some((instance, conn)));
            }
        }
        Ok(None)
    }

    pub fn instance_connection_url(
        &self,
        instance_name: &str,
        connection_name: &str,
    ) -> StoreResult<String> {
        let instance = self.get_managed_instance_by_name(instance_name)?;
        let conn = self.get_instance_connection(instance_name, connection_name)?;
        self.instance_connection_url_for_database(&instance, &conn, &conn.database)
    }

    pub fn instance_connection_url_for_names(
        &self,
        instance_name: &str,
        connection_name: &str,
        database: &str,
    ) -> StoreResult<String> {
        let instance = self.get_managed_instance_by_name(instance_name)?;
        let conn = self.get_instance_connection(instance_name, connection_name)?;
        self.instance_connection_url_for_database(&instance, &conn, database)
    }

    pub fn instance_connection_url_for_database(
        &self,
        instance: &ManagedInstance,
        conn: &InstanceConnection,
        database: &str,
    ) -> StoreResult<String> {
        let password = self.load_instance_connection_password(&conn.id)?;
        build_database_url_from_parts(
            &instance.host,
            instance.port,
            &conn.username,
            database,
            password.as_deref(),
            Some(conn.ssl_mode.as_str()),
        )
    }

    pub fn test_instance_connection<F, Fut>(
        &self,
        instance_name: &str,
        input: &NewInstanceConnection,
        ping_fn: F,
    ) -> StoreResult<ConnectionPrecheck>
    where
        F: FnOnce(&str) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
    {
        let instance = self.get_managed_instance_by_name(instance_name)?;
        run_connection_precheck(self, &instance, input, false, ping_fn)
    }

    /// Build precheck input from a saved connection, including decrypted password.
    pub fn new_instance_connection_from_saved(
        &self,
        instance_name: &str,
        connection_name: &str,
    ) -> StoreResult<NewInstanceConnection> {
        let conn = self.get_instance_connection(instance_name, connection_name)?;
        let password = self.load_instance_connection_password(&conn.id)?;
        Ok(NewInstanceConnection {
            name: conn.name,
            username: conn.username,
            database: conn.database,
            password,
            ssl_mode: Some(conn.ssl_mode),
            env_label: conn.env_label,
        })
    }

    /// Test a previously saved connection using credentials stored in the DB.
    pub fn test_saved_instance_connection<F, Fut>(
        &self,
        instance_name: &str,
        connection_name: &str,
        ping_fn: F,
    ) -> StoreResult<ConnectionPrecheck>
    where
        F: FnOnce(&str) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
    {
        let input = self.new_instance_connection_from_saved(instance_name, connection_name)?;
        self.test_instance_connection(instance_name, &input, ping_fn)
    }

    /// Test an edited connection's current values against the instance, borrowing
    /// the stored password when the password field is blank (not re-entered on
    /// edit). The name/username/database come from the form, so modified fields
    /// are actually exercised; only the blank password falls back to the saved
    /// one so an unchanged password still authenticates (matching the original
    /// dbm's edit-form test behavior).
    pub fn test_edited_instance_connection<F, Fut>(
        &self,
        instance_name: &str,
        original_name: &str,
        input: &NewInstanceConnection,
        ping_fn: F,
    ) -> StoreResult<ConnectionPrecheck>
    where
        F: FnOnce(&str) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
    {
        let lookup = if original_name.is_empty() {
            &input.name
        } else {
            original_name
        };
        let mut saved = self.new_instance_connection_from_saved(instance_name, lookup)?;
        saved.name = input.name.clone();
        saved.username = input.username.clone();
        saved.database = input.database.clone();
        self.test_instance_connection(instance_name, &saved, ping_fn)
    }

    /// Record the most recent connection test outcome: set `test_succeeded_at`
    /// on success or `test_failed_at` on failure.
    pub fn record_connection_test_result(
        &self,
        instance_name: &str,
        connection_name: &str,
        ok: bool,
    ) -> StoreResult<()> {
        let instance = self.get_managed_instance_by_name(instance_name)?;
        let column = if ok {
            "test_succeeded_at"
        } else {
            "test_failed_at"
        };
        self.sqlite().execute(
            &format!(
                "UPDATE instance_connections
                 SET {column} = datetime('now')
                 WHERE instance_id = ?1 AND name = ?2 COLLATE NOCASE"
            ),
            params![instance.id, connection_name],
        )?;
        Ok(())
    }

    pub fn add_instance_connection<F, Fut>(
        &self,
        instance_name: &str,
        input: NewInstanceConnection,
        ping_fn: F,
    ) -> StoreResult<InstanceConnection>
    where
        F: FnOnce(&str) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
    {
        let instance = self.get_managed_instance_by_name(instance_name)?;
        let precheck = run_connection_precheck(self, &instance, &input, true, ping_fn)?;
        if !precheck.ok {
            return Err(StoreError::PrecheckFailed {
                report: format_connection_precheck(&instance.name, &input.name, &precheck),
            });
        }

        if self
            .get_instance_connection(instance_name, &input.name)
            .is_ok()
        {
            return Err(StoreError::AlreadyExists(format!(
                "{}:{}",
                instance_name, input.name
            )));
        }

        let id = format!("ic_{}", uuid::Uuid::new_v4().simple());
        let (nonce, enc) = encrypt_password(input.password.as_deref())?;
        let ssl_mode = input
            .ssl_mode
            .clone()
            .unwrap_or_else(|| "prefer".to_string());

        self.sqlite().execute(
            "INSERT INTO instance_connections (
                id, instance_id, name, username, database_name,
                password_nonce, password_enc, ssl_mode, env_label
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                instance.id,
                input.name,
                input.username,
                input.database,
                nonce,
                enc,
                ssl_mode,
                input.env_label,
            ],
        )?;

        let conn = self.get_instance_connection(instance_name, &input.name)?;
        record_audit(
            self.sqlite(),
            "instance_connections.add",
            &format!("{instance_name}:{}", conn.name),
            json!({
                "instance": instance_name,
                "connection": conn.name,
            }),
        )?;
        Ok(conn)
    }

    pub fn delete_instance_connection(
        &self,
        instance_name: &str,
        connection_name: &str,
    ) -> StoreResult<bool> {
        let instance = self.get_managed_instance_by_name(instance_name)?;
        let conn = self.get_instance_connection(instance_name, connection_name)?;
        let affected = self.sqlite().execute(
            "DELETE FROM instance_connections WHERE id = ?1",
            params![conn.id],
        )?;
        if affected == 0 {
            return Ok(false);
        }
        record_audit(
            self.sqlite(),
            "instance_connections.delete",
            &format!("{instance_name}:{connection_name}"),
            json!({
                "instance": instance_name,
                "connection": connection_name,
            }),
        )?;
        let _ = instance;
        Ok(true)
    }

    pub fn update_instance_connection<F, Fut>(
        &self,
        instance_name: &str,
        connection_name: &str,
        patch: UpdateInstanceConnection,
        ping_fn: F,
    ) -> StoreResult<InstanceConnection>
    where
        F: FnOnce(&str) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
    {
        let instance = self.get_managed_instance_by_name(instance_name)?;
        let existing = self.get_instance_connection(instance_name, connection_name)?;

        let new_name = patch
            .name
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| existing.name.clone());

        if !connection_names_equal(&new_name, &existing.name)
            && self
                .get_instance_connection_for_instance(&instance, &new_name)
                .is_ok_and(|other| other.id != existing.id)
        {
            return Err(StoreError::AlreadyExists(format!(
                "{}:{new_name}",
                instance_name
            )));
        }

        let merged = NewInstanceConnection {
            name: new_name.clone(),
            username: patch.username.unwrap_or_else(|| existing.username.clone()),
            database: patch.database.unwrap_or_else(|| existing.database.clone()),
            password: match &patch.password {
                None => self.load_instance_connection_password(&existing.id)?,
                Some(inner) => inner.clone(),
            },
            ssl_mode: patch.ssl_mode.or_else(|| Some(existing.ssl_mode.clone())),
            env_label: patch.env_label.or(existing.env_label.clone()),
        };

        let precheck = run_connection_precheck(self, &instance, &merged, false, ping_fn)?;
        if !precheck.ok {
            return Err(StoreError::PrecheckFailed {
                report: format_connection_precheck(&instance.name, &existing.name, &precheck),
            });
        }

        let (nonce, enc) = encrypt_password(merged.password.as_deref())?;
        let ssl_mode = merged.ssl_mode.unwrap_or_else(|| existing.ssl_mode.clone());

        let affected = self.sqlite().execute(
            "UPDATE instance_connections
             SET name = ?1, username = ?2, database_name = ?3, password_nonce = ?4, password_enc = ?5,
                 ssl_mode = ?6, env_label = ?7, updated_at = datetime('now')
             WHERE id = ?8",
            params![
                merged.name,
                merged.username,
                merged.database,
                nonce,
                enc,
                ssl_mode,
                merged.env_label,
                existing.id,
            ],
        )?;
        if affected == 0 {
            return Err(StoreError::NotFound(format!(
                "connection `{connection_name}` on instance `{instance_name}`"
            )));
        }

        let conn = self.get_instance_connection_by_id(&instance, &existing.id)?;
        record_audit(
            self.sqlite(),
            "instance_connections.update",
            &format!("{instance_name}:{connection_name}"),
            json!({
                "instance": instance_name,
                "connection": connection_name,
                "new_name": if !connection_names_equal(&new_name, &existing.name) {
                    Some(new_name.clone())
                } else {
                    None::<String>
                },
            }),
        )?;
        Ok(conn)
    }

    pub(super) fn load_instance_connection_password(
        &self,
        connection_id: &str,
    ) -> StoreResult<Option<String>> {
        let (nonce, enc): (Option<Vec<u8>>, Option<Vec<u8>>) = self.sqlite().query_row(
            "SELECT password_nonce, password_enc FROM instance_connections WHERE id = ?1",
            params![connection_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        let (Some(nonce), Some(ciphertext)) = (nonce, enc) else {
            return Ok(None);
        };

        let secret = SecretBox::from_parts(nonce, ciphertext)?;
        Ok(Some(secret.decrypt()?))
    }
}

fn connection_names_equal(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn run_connection_precheck<F, Fut>(
    store: &super::Store,
    instance: &ManagedInstance,
    input: &NewInstanceConnection,
    check_name_taken: bool,
    ping_fn: F,
) -> StoreResult<ConnectionPrecheck>
where
    F: FnOnce(&str) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
{
    let mut issues = Vec::new();

    if input.name.trim().is_empty() {
        issues.push(issue(
            "NAME_EMPTY",
            PrecheckLevel::Error,
            "connection name is required",
        ));
    }
    if input.username.trim().is_empty() {
        issues.push(issue(
            "USERNAME_EMPTY",
            PrecheckLevel::Error,
            "username is required",
        ));
    }
    if input.database.trim().is_empty() {
        issues.push(issue(
            "DATABASE_EMPTY",
            PrecheckLevel::Error,
            "database is required",
        ));
    }

    if check_name_taken
        && store
            .get_instance_connection(&instance.name, &input.name)
            .is_ok()
    {
        issues.push(issue(
            "NAME_TAKEN",
            PrecheckLevel::Error,
            format!(
                "connection `{}` already exists on instance `{}`",
                input.name, instance.name
            ),
        ));
    }

    let ok_so_far = !issues.iter().any(|i| i.level == PrecheckLevel::Error);
    let mut version = None;

    if ok_so_far {
        match build_database_url_from_parts(
            &instance.host,
            instance.port,
            &input.username,
            &input.database,
            input.password.as_deref(),
            input.ssl_mode.as_deref(),
        ) {
            Ok(url) => match ping_database_url(&url, ping_fn) {
                Ok(v) => {
                    let full = v.lines().next().unwrap_or(&v).to_string();
                    version = Some(full.clone());
                    if let Err(err) = store.upsert_instance_version(&instance.id, &full) {
                        issues.push(issue(
                            "VERSION_PERSIST_FAILED",
                            PrecheckLevel::Warning,
                            format!("credentials ok but failed to persist version: {err}"),
                        ));
                    }
                    issues.push(issue(
                        "CREDENTIALS_OK",
                        PrecheckLevel::Info,
                        format!(
                            "SELECT 1 ok at {}:{} ({})",
                            instance.host,
                            instance.port,
                            version.as_deref().unwrap_or("ok")
                        ),
                    ));
                }
                Err(err) => issues.push(issue(
                    "CREDENTIALS_FAILED",
                    PrecheckLevel::Error,
                    format!(
                        "SELECT 1 at {}:{} failed: {err}",
                        instance.host, instance.port
                    ),
                )),
            },
            Err(err) => issues.push(issue(
                "CREDENTIALS_FAILED",
                PrecheckLevel::Error,
                err.to_string(),
            )),
        }
    }

    let ok = !issues.iter().any(|i| i.level == PrecheckLevel::Error);
    Ok(ConnectionPrecheck {
        ok,
        version,
        issues,
    })
}

fn issue(code: &'static str, level: PrecheckLevel, message: impl Into<String>) -> PrecheckIssue {
    PrecheckIssue {
        code,
        level,
        message: message.into(),
    }
}

fn ping_database_url<F, Fut>(url: &str, ping_fn: F) -> Result<String, String>
where
    F: FnOnce(&str) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
{
    let url = url.to_string();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| err.to_string())?;

        rt.block_on(async { ping_fn(&url).await })
    })
    .join()
    .map_err(|_| "connection precheck thread panicked".to_string())?
}

pub fn format_connection_precheck(
    instance_name: &str,
    connection_name: &str,
    precheck: &ConnectionPrecheck,
) -> String {
    let mut lines = vec![format!("{instance_name}:{connection_name}")];
    for issue in &precheck.issues {
        lines.push(format!(
            "  [{}] {}: {}",
            issue.level.as_str(),
            issue.code,
            issue.message
        ));
    }
    lines.join("\n")
}

fn map_instance_connection_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<InstanceConnection> {
    Ok(InstanceConnection {
        id: row.get(0)?,
        instance_id: row.get(1)?,
        name: row.get(2)?,
        username: row.get(3)?,
        database: row.get(4)?,
        has_password: row.get(5)?,
        ssl_mode: row.get(6)?,
        env_label: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        test_succeeded_at: row.get(10)?,
        test_failed_at: row.get(11)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_name_fails_precheck() {
        let store = super::super::Store::open_in_memory().unwrap();
        store
            .sqlite()
            .execute_batch(
                "INSERT INTO managed_instances (id, fingerprint, name, engine, host, port, registered_at)
                 VALUES ('inst_t', 'fp', 'pg', 'postgres', '127.0.0.1', 5432, datetime('now'));",
            )
            .unwrap();

        let precheck = store
            .test_instance_connection(
                "pg",
                &NewInstanceConnection {
                    name: "  ".into(),
                    username: "u".into(),
                    database: "postgres".into(),
                    password: None,
                    ssl_mode: None,
                    env_label: None,
                },
                |_url| async { Err("not reached".to_string()) },
            )
            .unwrap();
        assert!(!precheck.ok);
        assert!(precheck.issues.iter().any(|i| i.code == "NAME_EMPTY"));
    }

    #[test]
    fn saved_connection_input_includes_decrypted_password() {
        let store = super::super::Store::open_in_memory().unwrap();
        store
            .sqlite()
            .execute_batch(
                "INSERT INTO managed_instances (id, fingerprint, name, engine, host, port, registered_at)
                 VALUES ('inst_t', 'fp', 'pg', 'postgres', '192.168.64.2', 5432, datetime('now'));",
            )
            .unwrap();
        let (nonce, enc) = crate::store::encrypt_password(Some("test")).unwrap();
        store
            .sqlite()
            .execute(
                "INSERT INTO instance_connections (
                    id, instance_id, name, username, database_name,
                    password_nonce, password_enc, ssl_mode, env_label
                 ) VALUES ('ic_t', 'inst_t', 'admin', 'test', 'postgres', ?1, ?2, 'prefer', 'dev')",
                params![nonce, enc],
            )
            .unwrap();

        let input = store
            .new_instance_connection_from_saved("pg", "admin")
            .unwrap();
        assert_eq!(input.name, "admin");
        assert_eq!(input.username, "test");
        assert_eq!(input.database, "postgres");
        assert_eq!(input.password.as_deref(), Some("test"));
        assert_eq!(input.ssl_mode.as_deref(), Some("prefer"));
        assert_eq!(input.env_label.as_deref(), Some("dev"));
    }

    #[test]
    fn renamed_connection_fetched_by_new_name() {
        let store = super::super::Store::open_in_memory().unwrap();
        store
            .sqlite()
            .execute_batch(
                "INSERT INTO managed_instances (id, fingerprint, name, engine, host, port, registered_at)
                 VALUES ('inst_t', 'fp', 'pg', 'postgres', '127.0.0.1', 5432, datetime('now'));
                 INSERT INTO instance_connections (
                    id, instance_id, name, username, database_name, ssl_mode
                 ) VALUES ('ic_t', 'inst_t', 'oldname', 'u', 'postgres', 'prefer');",
            )
            .unwrap();

        store
            .sqlite()
            .execute(
                "UPDATE instance_connections SET name = 'newname' WHERE id = 'ic_t'",
                [],
            )
            .unwrap();

        let conn = store.get_instance_connection("pg", "newname").unwrap();
        assert_eq!(conn.name, "newname");
        assert!(store.get_instance_connection("pg", "oldname").is_err());
    }

    #[test]
    fn delete_writes_audit_log() {
        let store = super::super::Store::open_in_memory().unwrap();
        store
            .sqlite()
            .execute_batch(
                "INSERT INTO managed_instances (id, fingerprint, name, engine, host, port, registered_at)
                 VALUES ('inst_t', 'fp', 'pg', 'postgres', '127.0.0.1', 5432, datetime('now'));
                 INSERT INTO instance_connections (
                    id, instance_id, name, username, database_name, ssl_mode
                 ) VALUES ('ic_t', 'inst_t', 'admin', 'u', 'postgres', 'prefer');",
            )
            .unwrap();

        assert!(store.delete_instance_connection("pg", "admin").unwrap());

        let action: String = store
            .sqlite()
            .query_row("SELECT action FROM audit_log", [], |row| row.get(0))
            .unwrap();
        assert_eq!(action, "instance_connections.delete");
    }

    #[test]
    fn record_test_result_sets_timestamps() {
        let store = super::super::Store::open_in_memory().unwrap();
        store
            .sqlite()
            .execute_batch(
                "INSERT INTO managed_instances (id, fingerprint, name, engine, host, port, registered_at)
                 VALUES ('inst_t', 'fp', 'pg', 'postgres', '127.0.0.1', 5432, datetime('now'));
                 INSERT INTO instance_connections (
                    id, instance_id, name, username, database_name, ssl_mode, updated_at
                 ) VALUES ('ic_t', 'inst_t', 'main', 'u', 'postgres', 'prefer', '2000-01-01 00:00:00');",
            )
            .unwrap();

        // A successful test records test_succeeded_at, not test_failed_at, and
        // must not touch updated_at (a test is not an edit).
        store
            .record_connection_test_result("pg", "main", true)
            .unwrap();
        let conns = store.list_instance_connections("pg").unwrap();
        assert!(conns[0].test_succeeded_at.is_some());
        assert!(conns[0].test_failed_at.is_none());
        assert_eq!(
            conns[0].updated_at, "2000-01-01 00:00:00",
            "test must not update updated_at"
        );

        // A later failed test records test_failed_at, still not updated_at.
        store
            .record_connection_test_result("pg", "main", false)
            .unwrap();
        let conns = store.list_instance_connections("pg").unwrap();
        assert!(conns[0].test_failed_at.is_some());
        assert!(conns[0].test_succeeded_at.is_some());
        assert_eq!(
            conns[0].updated_at, "2000-01-01 00:00:00",
            "test must not update updated_at"
        );
    }

    #[test]
    fn edited_test_uses_form_values_but_stored_password() {
        let store = super::super::Store::open_in_memory().unwrap();
        store
            .sqlite()
            .execute_batch(
                "INSERT INTO managed_instances (id, fingerprint, name, engine, host, port, registered_at)
                 VALUES ('inst_t', 'fp', 'pg', 'postgres', '192.168.64.2', 5432, datetime('now'));
                 INSERT INTO instance_connections (
                    id, instance_id, name, username, database_name, ssl_mode, updated_at
                 ) VALUES ('ic_t', 'inst_t', 'main', 'olduser', 'postgres', 'prefer', '2000-01-01 00:00:00');",
            )
            .unwrap();
        let (nonce, enc) = crate::store::encrypt_password(Some("secretpw")).unwrap();
        store
            .sqlite()
            .execute(
                "UPDATE instance_connections SET password_nonce = ?1, password_enc = ?2 WHERE id = 'ic_t'",
                params![nonce, enc],
            )
            .unwrap();

        // The edited form changed the username to 'newuser' and left the
        // password blank. The precheck input must carry the new username and
        // database but inherit the stored password ('secretpw') so an unchanged
        // password still authenticates while modified fields are exercised.
        let input = NewInstanceConnection {
            name: "renamed".into(),
            username: "newuser".into(),
            database: "appdb".into(),
            password: None, // blank -> borrow the stored password
            ssl_mode: None,
            env_label: None,
        };
        let probe = |url: &str| {
            let url = url.to_string();
            async move {
                // The DSN embeds username/password; assert they reflect the
                // modified username and the borrowed stored password.
                assert!(
                    url.contains("newuser") && url.contains("secretpw"),
                    "expected modified user + stored password in DSN, got {url}"
                );
                Err("probe reached connection build".to_string())
            }
        };
        let precheck = store
            .test_edited_instance_connection("pg", "main", &input, probe)
            .unwrap();
        assert!(!precheck.ok, "probe always fails before reaching the DB");
    }
}
