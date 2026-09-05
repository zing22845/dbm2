use std::collections::HashMap;

use dbm_discovery::{
    DiscoveredInstance, DiscoveryConfig, DiscoveryScanner, ScanOptions, ScanResult,
};
use rusqlite::{OptionalExtension, params};

use crate::instance::ManagedInstance;
use crate::register_precheck::{RegisterOptions, RegisterResult, ensure_prechecks_pass};
use crate::{StoreError, StoreResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunDiscoveryOptions {
    pub include_managed_hosts: bool,
}

impl super::Store {
    pub fn registered_fingerprints(&self) -> StoreResult<HashMap<String, String>> {
        let mut stmt = self
            .sqlite()
            .prepare("SELECT fingerprint, id FROM managed_instances")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
    }

    pub fn instance_scan_hosts(&self) -> StoreResult<Vec<String>> {
        let mut stmt = self
            .sqlite()
            .prepare("SELECT DISTINCT host FROM managed_instances ORDER BY host ASC")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn run_discovery(&self, config: DiscoveryConfig) -> StoreResult<ScanResult> {
        self.run_discovery_with_options(
            config,
            RunDiscoveryOptions {
                include_managed_hosts: false,
            },
        )
    }

    pub fn run_discovery_with_options(
        &self,
        config: DiscoveryConfig,
        opts: RunDiscoveryOptions,
    ) -> StoreResult<ScanResult> {
        let config = self.prepare_discovery_config(config, opts)?;
        self.run_discovery_scan(&config, &DiscoveryScanner::postgres_default())
    }

    pub fn run_discovery_with_scan_options(
        &self,
        config: DiscoveryConfig,
        opts: RunDiscoveryOptions,
        scan_options: ScanOptions<'_>,
    ) -> StoreResult<ScanResult> {
        let config = self.prepare_discovery_config(config, opts)?;
        self.run_discovery_scan_with_options(
            &config,
            &DiscoveryScanner::postgres_default(),
            scan_options,
        )
    }

    fn prepare_discovery_config(
        &self,
        mut config: DiscoveryConfig,
        opts: RunDiscoveryOptions,
    ) -> StoreResult<DiscoveryConfig> {
        if opts.include_managed_hosts {
            config = config.with_extra_hosts(self.instance_scan_hosts()?);
        }
        Ok(config)
    }

    pub fn run_discovery_scan(
        &self,
        config: &DiscoveryConfig,
        scanner: &DiscoveryScanner,
    ) -> StoreResult<ScanResult> {
        self.run_discovery_scan_with_options(
            config,
            scanner,
            ScanOptions {
                progress: None,
                cancel: None,
            },
        )
    }

    pub fn run_discovery_scan_with_options(
        &self,
        config: &DiscoveryConfig,
        scanner: &DiscoveryScanner,
        scan_options: ScanOptions<'_>,
    ) -> StoreResult<ScanResult> {
        let registered = self.registered_fingerprints()?;
        let result = scanner.scan_with_options(config, &registered, scan_options);
        let cancelled = scan_options
            .cancel
            .is_some_and(|cancel| cancel.load(std::sync::atomic::Ordering::Relaxed));
        if !cancelled {
            self.persist_scan(&result)?;
        }
        Ok(result)
    }

    pub fn list_discovered(&self, unregistered_only: bool) -> StoreResult<Vec<DiscoveredInstance>> {
        let conn = self.sqlite();
        let scan_id: Option<String> = conn
            .query_row(
                "SELECT id FROM discovery_scans ORDER BY completed_at DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;

        let Some(scan_id) = scan_id else {
            return Ok(Vec::new());
        };

        let sql = if unregistered_only {
            "SELECT discovery_id, fingerprint, engine, host, port, socket_path, data_dir,
                    systemd_unit, version, status, sources_json, confidence,
                    already_registered, registered_instance_id, scanned_at
             FROM discovery_cache
             WHERE scan_id = ?1 AND already_registered = 0
             ORDER BY host, port"
        } else {
            "SELECT discovery_id, fingerprint, engine, host, port, socket_path, data_dir,
                    systemd_unit, version, status, sources_json, confidence,
                    already_registered, registered_instance_id, scanned_at
             FROM discovery_cache
             WHERE scan_id = ?1
             ORDER BY already_registered ASC, host, port"
        };

        let mut stmt = self.sqlite().prepare(sql)?;
        let rows = stmt.query_map(params![scan_id], map_discovered_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn register_discovered(
        &self,
        discovery_ids: &[String],
        options: RegisterOptions,
    ) -> StoreResult<RegisterResult> {
        let prechecks = self.precheck_register(discovery_ids, &options)?;
        ensure_prechecks_pass(&prechecks, options.force)?;

        let mut registered = Vec::new();
        for (idx, discovery_id) in discovery_ids.iter().enumerate() {
            let item = self.get_cached_discovery(discovery_id)?;
            if item.already_registered {
                return Err(StoreError::Other(format!(
                    "instance `{}` ({}) is already registered",
                    item.host, item.port
                )));
            }

            let inst_name = options.name.clone().unwrap_or_else(|| {
                if discovery_ids.len() == 1 {
                    default_instance_name(&item)
                } else {
                    format!("{}-{}", default_instance_name(&item), idx + 1)
                }
            });

            let id = format!("inst_{}", uuid::Uuid::new_v4().simple());
            self.sqlite().execute(
                "INSERT INTO managed_instances (
                    id, fingerprint, name, engine, host, port, socket_path, data_dir, env_label
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    id,
                    item.fingerprint,
                    inst_name,
                    item.engine.to_string(),
                    item.host,
                    i64::from(item.port),
                    item.socket_path,
                    item.data_dir,
                    None::<String>,
                ],
            )?;

            self.probe_and_upsert_instance_lifecycle(&id)?;
            registered.push(self.get_managed_instance_by_id(&id)?);

            self.sqlite().execute(
                "UPDATE discovery_cache
                 SET already_registered = 1, registered_instance_id = ?1
                 WHERE fingerprint = ?2",
                params![id, item.fingerprint],
            )?;
        }
        Ok(RegisterResult {
            instances: registered,
            prechecks,
        })
    }

    pub fn list_managed_instances(&self) -> StoreResult<Vec<ManagedInstance>> {
        let mut stmt = self.sqlite().prepare(
            "SELECT id, fingerprint, name, engine, host, port, socket_path, data_dir, env_label, registered_at,
                    version_full, version_short, version_checked_at,
                    lifecycle_status, lifecycle_checked_at, lifecycle_detail
             FROM managed_instances
             ORDER BY name ASC",
        )?;
        let rows = stmt.query_map([], map_managed_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn unregister_managed(&self, name: &str) -> StoreResult<bool> {
        let affected = self.sqlite().execute(
            "DELETE FROM managed_instances WHERE name = ?1 COLLATE NOCASE",
            params![name],
        )?;
        Ok(affected > 0)
    }

    fn persist_scan(&self, scan: &ScanResult) -> StoreResult<()> {
        let tx = self.sqlite().unchecked_transaction()?;
        tx.execute(
            "INSERT INTO discovery_scans (id, started_at, completed_at, instance_count)
             VALUES (?1, datetime('now'), datetime('now'), ?2)",
            params![
                scan.scan_id,
                i64::try_from(scan.instances.len()).unwrap_or(0)
            ],
        )?;

        for item in &scan.instances {
            tx.execute(
                "INSERT INTO discovery_cache (
                    discovery_id, scan_id, fingerprint, engine, host, port, socket_path, data_dir,
                    systemd_unit, version, status, sources_json, confidence,
                    already_registered, registered_instance_id, scanned_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    item.discovery_id,
                    scan.scan_id,
                    item.fingerprint,
                    item.engine.to_string(),
                    item.host,
                    i64::from(item.port),
                    item.socket_path,
                    item.data_dir,
                    item.systemd_unit,
                    item.version,
                    item.status.as_str(),
                    serde_json::to_string(&item.sources)
                        .map_err(|e| StoreError::Other(e.to_string()))?,
                    item.confidence.as_str(),
                    i64::from(item.already_registered),
                    item.registered_instance_id,
                    item.scanned_at,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub(super) fn get_cached_discovery(
        &self,
        discovery_id: &str,
    ) -> StoreResult<DiscoveredInstance> {
        self.sqlite()
            .query_row(
                "SELECT discovery_id, fingerprint, engine, host, port, socket_path, data_dir,
                        systemd_unit, version, status, sources_json, confidence,
                        already_registered, registered_instance_id, scanned_at
                 FROM discovery_cache WHERE discovery_id = ?1",
                params![discovery_id],
                map_discovered_row,
            )
            .map_err(|_| StoreError::NotFound(discovery_id.to_string()))
    }

    pub fn get_managed_instance_by_name(&self, name: &str) -> StoreResult<ManagedInstance> {
        self.sqlite()
            .query_row(
                "SELECT id, fingerprint, name, engine, host, port, socket_path, data_dir, env_label, registered_at,
                        version_full, version_short, version_checked_at,
                        lifecycle_status, lifecycle_checked_at, lifecycle_detail
                 FROM managed_instances WHERE name = ?1 COLLATE NOCASE",
                params![name],
                map_managed_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("instance `{name}`")))
    }

    pub(crate) fn get_managed_instance_by_id(&self, id: &str) -> StoreResult<ManagedInstance> {
        self.sqlite()
            .query_row(
                "SELECT id, fingerprint, name, engine, host, port, socket_path, data_dir, env_label, registered_at,
                        version_full, version_short, version_checked_at,
                        lifecycle_status, lifecycle_checked_at, lifecycle_detail
                 FROM managed_instances WHERE id = ?1",
                params![id],
                map_managed_row,
            )
            .map_err(Into::into)
    }

    /// Persist `SELECT version()` on the instance when the full string changed.
    /// Returns `true` if a row was updated.
    pub fn upsert_instance_version(
        &self,
        instance_id: &str,
        version_raw: &str,
    ) -> StoreResult<bool> {
        let full = version_raw
            .lines()
            .next()
            .unwrap_or(version_raw)
            .trim()
            .to_string();
        let short = crate::parse_version_short(&full);

        let current: Option<Option<String>> = self
            .sqlite()
            .query_row(
                "SELECT version_full FROM managed_instances WHERE id = ?1",
                params![instance_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?;

        if current.flatten().as_deref() == Some(full.as_str()) {
            return Ok(false);
        }

        let n = self.sqlite().execute(
            "UPDATE managed_instances
             SET version_full = ?1,
                 version_short = ?2,
                 version_checked_at = datetime('now')
             WHERE id = ?3",
            params![full, short, instance_id],
        )?;
        Ok(n > 0)
    }

    pub fn upsert_instance_lifecycle(
        &self,
        instance_id: &str,
        status: crate::LifecycleStatus,
        detail: Option<&str>,
    ) -> StoreResult<()> {
        self.sqlite().execute(
            "UPDATE managed_instances
             SET lifecycle_status = ?1,
                 lifecycle_detail = ?2,
                 lifecycle_checked_at = datetime('now')
             WHERE id = ?3",
            params![status.as_str(), detail, instance_id],
        )?;
        Ok(())
    }

    pub fn probe_and_upsert_instance_lifecycle(
        &self,
        instance_id: &str,
    ) -> StoreResult<crate::LifecycleStatus> {
        let inst = self.get_managed_instance_by_id(instance_id)?;
        let (status, detail) = crate::probe_lifecycle_status(inst.data_dir.as_deref());
        self.upsert_instance_lifecycle(instance_id, status, detail)?;
        Ok(status)
    }
}

fn default_instance_name(item: &DiscoveredInstance) -> String {
    // IPv6 hosts are stored unbracketed (`::1`); bracket them so host:port stays unambiguous.
    if item.host.contains(':') {
        format!("[{}]:{}", item.host, item.port)
    } else {
        format!("{}:{}", item.host, item.port)
    }
}

fn map_discovered_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DiscoveredInstance> {
    let engine_raw: String = row.get(2)?;
    let engine = match engine_raw.as_str() {
        "postgres" => dbm_core::Engine::Postgres,
        other => {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "unknown engine `{other}`"
            )));
        }
    };
    let sources_json: String = row.get(10)?;
    let sources: Vec<dbm_discovery::DiscoverySource> =
        serde_json::from_str(&sources_json).unwrap_or_default();
    let confidence = parse_confidence(&row.get::<_, String>(11)?);
    let status = parse_status(&row.get::<_, String>(9)?);

    Ok(DiscoveredInstance {
        discovery_id: row.get(0)?,
        fingerprint: row.get(1)?,
        engine,
        host: row.get(3)?,
        port: u16::try_from(row.get::<_, i64>(4)?).unwrap_or(5432),
        socket_path: row.get(5)?,
        data_dir: row.get(6)?,
        systemd_unit: row.get(7)?,
        version: row.get(8)?,
        status,
        sources,
        confidence,
        already_registered: row.get::<_, i64>(12)? != 0,
        registered_instance_id: row.get(13)?,
        scanned_at: row.get(14)?,
    })
}

fn map_managed_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ManagedInstance> {
    let engine_raw: String = row.get(3)?;
    let engine = match engine_raw.as_str() {
        "postgres" => dbm_core::Engine::Postgres,
        other => {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "unknown engine `{other}`"
            )));
        }
    };

    Ok(ManagedInstance {
        id: row.get(0)?,
        fingerprint: row.get(1)?,
        name: row.get(2)?,
        engine,
        host: row.get(4)?,
        port: u16::try_from(row.get::<_, i64>(5)?).unwrap_or(5432),
        socket_path: row.get(6)?,
        data_dir: row.get(7)?,
        env_label: row.get(8)?,
        registered_at: row.get(9)?,
        version_full: row.get(10)?,
        version_short: row.get(11)?,
        version_checked_at: row.get(12)?,
        lifecycle_status: row.get(13)?,
        lifecycle_checked_at: row.get(14)?,
        lifecycle_detail: row.get(15)?,
    })
}

fn parse_confidence(raw: &str) -> dbm_discovery::Confidence {
    match raw {
        "high" => dbm_discovery::Confidence::High,
        "low" => dbm_discovery::Confidence::Low,
        _ => dbm_discovery::Confidence::Medium,
    }
}

fn parse_status(raw: &str) -> dbm_discovery::InstanceRunStatus {
    match raw {
        "running" => dbm_discovery::InstanceRunStatus::Running,
        "stopped" => dbm_discovery::InstanceRunStatus::Stopped,
        _ => dbm_discovery::InstanceRunStatus::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use dbm_discovery::{
        Confidence, DiscoveredInstance, DiscoveryConfig, DiscoveryScanner, DiscoverySource,
        InstanceRunStatus, ScanOptions, ScanResult,
    };

    #[test]
    fn discovery_excludes_managed_hosts_by_default() {
        let store = super::super::Store::open_in_memory().unwrap();
        store
            .sqlite()
            .execute(
                "INSERT INTO managed_instances (id, fingerprint, name, engine, host, port, registered_at)
                 VALUES ('inst_t', 'fp', 'remote-pg', 'postgres', '192.168.64.17', 5432, datetime('now'))",
                [],
            )
            .unwrap();

        let config = store
            .prepare_discovery_config(
                DiscoveryConfig::default(),
                super::RunDiscoveryOptions {
                    include_managed_hosts: false,
                },
            )
            .unwrap();

        assert_eq!(config.hosts, vec!["127.0.0.1"]);
    }

    #[test]
    fn discovery_includes_managed_hosts_when_requested() {
        let store = super::super::Store::open_in_memory().unwrap();
        store
            .sqlite()
            .execute(
                "INSERT INTO managed_instances (id, fingerprint, name, engine, host, port, registered_at)
                 VALUES ('inst_t', 'fp', 'remote-pg', 'postgres', '192.168.64.17', 5432, datetime('now'))",
                [],
            )
            .unwrap();

        let config = store
            .prepare_discovery_config(
                DiscoveryConfig::default(),
                super::RunDiscoveryOptions {
                    include_managed_hosts: true,
                },
            )
            .unwrap();

        assert_eq!(config.hosts, vec!["127.0.0.1", "192.168.64.17"]);
    }

    #[test]
    fn scan_persist_list_and_register() {
        let store = super::super::Store::open_in_memory().unwrap();
        let config = DiscoveryConfig {
            targets: vec![],
            hosts: vec!["127.0.0.1".into()],
            ports: vec![1],
            max_duration: std::time::Duration::from_secs(1),
            engine: dbm_core::Engine::Postgres,
        };

        let scan = store
            .run_discovery_scan(&config, &DiscoveryScanner::port_only())
            .unwrap();
        assert!(scan.instances.is_empty());
        assert!(store.list_discovered(false).unwrap().is_empty());
        assert!(store.list_managed_instances().unwrap().is_empty());
    }

    #[test]
    fn cancelled_scan_keeps_the_previous_discovery_cache() {
        let store = super::super::Store::open_in_memory().unwrap();
        store
            .persist_scan(&ScanResult {
                scan_id: "before-cancel".into(),
                instances: vec![DiscoveredInstance {
                    discovery_id: "cached-instance".into(),
                    fingerprint: "cached-fingerprint".into(),
                    engine: dbm_core::Engine::Postgres,
                    host: "127.0.0.1".into(),
                    port: 5432,
                    socket_path: None,
                    data_dir: None,
                    systemd_unit: None,
                    version: None,
                    status: InstanceRunStatus::Running,
                    sources: vec![DiscoverySource::Port],
                    confidence: Confidence::High,
                    already_registered: false,
                    registered_instance_id: None,
                    scanned_at: "now".into(),
                }],
            })
            .unwrap();
        let cancel = AtomicBool::new(true);
        let config = DiscoveryConfig {
            targets: vec![],
            hosts: vec!["127.0.0.1".into()],
            ports: vec![1],
            max_duration: std::time::Duration::from_secs(1),
            engine: dbm_core::Engine::Postgres,
        };

        store
            .run_discovery_scan_with_options(
                &config,
                &DiscoveryScanner::port_only(),
                ScanOptions {
                    progress: None,
                    cancel: Some(&cancel),
                },
            )
            .unwrap();

        let scan_count: i64 = store
            .sqlite()
            .query_row("SELECT COUNT(*) FROM discovery_scans", [], |row| row.get(0))
            .unwrap();
        assert_eq!(scan_count, 1);
        let cached = store.list_discovered(false).unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].discovery_id, "cached-instance");
    }

    #[test]
    fn upsert_instance_version_sets_and_skips_unchanged() {
        let store = super::super::Store::open_in_memory().unwrap();
        store
            .sqlite()
            .execute(
                "INSERT INTO managed_instances (id, fingerprint, name, engine, host, port, registered_at)
                 VALUES ('inst_v', 'fp_v', 'pg-v', 'postgres', '127.0.0.1', 5432, datetime('now'))",
                [],
            )
            .unwrap();

        let full = "PostgreSQL 16.2 on x86_64-test";
        assert!(store.upsert_instance_version("inst_v", full).unwrap());
        let inst = store.get_managed_instance_by_name("pg-v").unwrap();
        assert_eq!(inst.version_full.as_deref(), Some(full));
        assert_eq!(inst.version_short.as_deref(), Some("16.2"));
        assert!(inst.version_checked_at.is_some());

        assert!(!store.upsert_instance_version("inst_v", full).unwrap());

        let full2 = "PostgreSQL 17.0 on x86_64-test";
        assert!(store.upsert_instance_version("inst_v", full2).unwrap());
        let inst = store.get_managed_instance_by_name("pg-v").unwrap();
        assert_eq!(inst.version_short.as_deref(), Some("17.0"));
    }

    #[test]
    fn probe_and_upsert_sets_ready_for_writable_data_dir() {
        let store = super::super::Store::open_in_memory().unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().to_str().unwrap();
        store
            .sqlite()
            .execute(
                "INSERT INTO managed_instances (id, fingerprint, name, engine, host, port, data_dir, registered_at)
                 VALUES ('inst_lc', 'fp_lc', 'pg-lc', 'postgres', '127.0.0.1', 5432, ?1, datetime('now'))",
                rusqlite::params![dir],
            )
            .unwrap();

        let status = store
            .probe_and_upsert_instance_lifecycle("inst_lc")
            .unwrap();
        assert_eq!(status, crate::LifecycleStatus::Ready);
        let inst = store.get_managed_instance_by_name("pg-lc").unwrap();
        assert_eq!(inst.lifecycle_status.as_deref(), Some("ready"));
        assert!(inst.lifecycle_checked_at.is_some());
        assert!(inst.lifecycle_detail.is_none());
    }

    #[test]
    fn probe_and_upsert_none_without_data_dir() {
        let store = super::super::Store::open_in_memory().unwrap();
        store
            .sqlite()
            .execute(
                "INSERT INTO managed_instances (id, fingerprint, name, engine, host, port, registered_at)
                 VALUES ('inst_n', 'fp_n', 'pg-n', 'postgres', '127.0.0.1', 5432, datetime('now'))",
                [],
            )
            .unwrap();
        let status = store.probe_and_upsert_instance_lifecycle("inst_n").unwrap();
        assert_eq!(status, crate::LifecycleStatus::None);
        let inst = store.get_managed_instance_by_name("pg-n").unwrap();
        assert_eq!(inst.lifecycle_status.as_deref(), Some("none"));
        assert_eq!(inst.lifecycle_detail.as_deref(), Some("NO_DATA_DIR"));
    }

    fn sample_discovered(host: &str, port: u16, data_dir: Option<&str>) -> DiscoveredInstance {
        DiscoveredInstance {
            discovery_id: "dsc_name".into(),
            fingerprint: "fp_name".into(),
            engine: dbm_core::Engine::Postgres,
            host: host.into(),
            port,
            socket_path: None,
            data_dir: data_dir.map(str::to_string),
            systemd_unit: None,
            version: None,
            status: InstanceRunStatus::Running,
            sources: vec![DiscoverySource::Port],
            confidence: Confidence::High,
            already_registered: false,
            registered_instance_id: None,
            scanned_at: "now".into(),
        }
    }

    #[test]
    fn default_instance_name_is_host_port() {
        let item = sample_discovered("127.0.0.1", 5432, Some("/var/lib/postgresql/16/main"));
        assert_eq!(super::default_instance_name(&item), "127.0.0.1:5432");
    }

    #[test]
    fn default_instance_name_brackets_ipv6() {
        let item = sample_discovered("::1", 5432, None);
        assert_eq!(super::default_instance_name(&item), "[::1]:5432");
    }
}
