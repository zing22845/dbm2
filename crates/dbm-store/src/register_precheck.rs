use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use dbm_discovery::DiscoveredInstance;

use crate::{StoreError, StoreResult};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrecheckLevel {
    Error,
    Warning,
    Info,
}

impl PrecheckLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warning => "WARN",
            Self::Info => "INFO",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrecheckIssue {
    pub code: &'static str,
    pub level: PrecheckLevel,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct InstancePrecheck {
    pub discovery_id: String,
    pub target: String,
    pub issues: Vec<PrecheckIssue>,
}

impl InstancePrecheck {
    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.level == PrecheckLevel::Error)
    }

    pub fn has_warnings(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.level == PrecheckLevel::Warning)
    }

    pub fn ok_to_register(&self, force: bool) -> bool {
        !self.has_errors() && (force || !self.has_warnings())
    }
}

#[derive(Debug, Clone, Default)]
pub struct RegisterOptions {
    pub name: Option<String>,
    /// Register despite warnings (errors still block).
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct RegisterResult {
    pub instances: Vec<crate::ManagedInstance>,
    pub prechecks: Vec<InstancePrecheck>,
}

pub fn format_precheck_report(checks: &[InstancePrecheck]) -> String {
    let mut lines = Vec::new();
    for check in checks {
        lines.push(format!("{} ({})", check.target, check.discovery_id));
        for issue in &check.issues {
            lines.push(format!(
                "  [{}] {}: {}",
                issue.level.as_str(),
                issue.code,
                issue.message
            ));
        }
    }
    lines.join("\n")
}

impl super::Store {
    pub fn precheck_register(
        &self,
        discovery_ids: &[String],
        options: &RegisterOptions,
    ) -> StoreResult<Vec<InstancePrecheck>> {
        let mut checks = Vec::with_capacity(discovery_ids.len());
        for discovery_id in discovery_ids {
            let item = self.get_cached_discovery(discovery_id)?;
            checks.push(run_precheck(self, &item, options, discovery_ids.len())?);
        }
        Ok(checks)
    }
}

fn run_precheck(
    store: &super::Store,
    item: &DiscoveredInstance,
    options: &RegisterOptions,
    batch_size: usize,
) -> StoreResult<InstancePrecheck> {
    let mut issues = Vec::new();

    if item.already_registered {
        issues.push(PrecheckIssue {
            code: "ALREADY_REGISTERED",
            level: PrecheckLevel::Error,
            message: format!("instance {}:{} is already registered", item.host, item.port),
        });
    }

    if fingerprint_registered(store, &item.fingerprint)? {
        issues.push(PrecheckIssue {
            code: "DUPLICATE_FINGERPRINT",
            level: PrecheckLevel::Error,
            message: "fingerprint already exists in managed_instances".into(),
        });
    }

    if let Some(name) = &options.name
        && batch_size == 1
        && managed_name_exists(store, name)?
    {
        issues.push(PrecheckIssue {
            code: "NAME_TAKEN",
            level: PrecheckLevel::Error,
            message: format!("managed instance name `{name}` already exists"),
        });
    }

    if let Some(issue) = check_reachability(item) {
        issues.push(issue);
    }

    issues.extend(check_data_dir(item.data_dir.as_deref()));
    issues.extend(check_systemd(item.systemd_unit.as_deref()));

    issues.push(PrecheckIssue {
        code: "LIFECYCLE_ONLY",
        level: PrecheckLevel::Info,
        message: "instance registered without credentials; add a connection in Instances view"
            .into(),
    });

    Ok(InstancePrecheck {
        discovery_id: item.discovery_id.clone(),
        target: format!("{}:{}", item.host, item.port),
        issues,
    })
}

fn fingerprint_registered(store: &super::Store, fingerprint: &str) -> StoreResult<bool> {
    let count: i64 = store.sqlite().query_row(
        "SELECT COUNT(*) FROM managed_instances WHERE fingerprint = ?1",
        rusqlite::params![fingerprint],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn managed_name_exists(store: &super::Store, name: &str) -> StoreResult<bool> {
    let count: i64 = store.sqlite().query_row(
        "SELECT COUNT(*) FROM managed_instances WHERE name = ?1 COLLATE NOCASE",
        rusqlite::params![name],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub(crate) fn check_reachability(item: &DiscoveredInstance) -> Option<PrecheckIssue> {
    if let Some(socket) = &item.socket_path {
        let path = Path::new(socket);
        if path.exists() && std::fs::metadata(path).is_ok() {
            return None;
        }
    }

    if tcp_reachable(&item.host, item.port) {
        return None;
    }

    Some(PrecheckIssue {
        code: "UNREACHABLE",
        level: PrecheckLevel::Warning,
        message: format!(
            "cannot connect to {}:{} or access socket `{}`; use --force to register anyway",
            item.host,
            item.port,
            item.socket_path.as_deref().unwrap_or("-"),
        ),
    })
}

pub(crate) fn tcp_reachable(host: &str, port: u16) -> bool {
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    for addr in addrs {
        if TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).is_ok() {
            return true;
        }
    }
    false
}

pub(crate) fn check_data_dir(data_dir: Option<&str>) -> Vec<PrecheckIssue> {
    let Some(data_dir) = data_dir else {
        return vec![PrecheckIssue {
            code: "NO_DATA_DIR",
            level: PrecheckLevel::Warning,
            message: "data_dir unknown; lifecycle via pg_ctl may be unavailable".into(),
        }];
    };

    let path = Path::new(data_dir);
    if !path.exists() {
        return vec![PrecheckIssue {
            code: "DATA_DIR_MISSING",
            level: PrecheckLevel::Warning,
            message: format!("data_dir `{data_dir}` does not exist"),
        }];
    }

    let mut issues = Vec::new();
    if path.is_dir() && std::fs::read_dir(path).is_err() {
        issues.push(PrecheckIssue {
            code: "DATA_DIR_NOT_READABLE",
            level: PrecheckLevel::Warning,
            message: format!("cannot read data_dir `{data_dir}`"),
        });
    }
    if path.is_dir() && !dir_writable(path) {
        issues.push(PrecheckIssue {
            code: "DATA_DIR_NOT_WRITABLE",
            level: PrecheckLevel::Warning,
            message: format!(
                "data_dir `{data_dir}` is not writable; lifecycle operations may fail"
            ),
        });
    }
    issues
}

fn dir_writable(path: &Path) -> bool {
    let probe = path.join(".dbm_write_probe");
    match std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}

pub(crate) fn check_systemd(systemd_unit: Option<&str>) -> Vec<PrecheckIssue> {
    let Some(unit) = systemd_unit else {
        return Vec::new();
    };

    let output = Command::new("systemctl").args(["is-active", unit]).output();

    match output {
        Ok(result) if result.status.success() => Vec::new(),
        Ok(result) => {
            let detail = String::from_utf8_lossy(&result.stderr).trim().to_string();
            vec![PrecheckIssue {
                code: "SYSTEMD_NOT_ACTIVE",
                level: PrecheckLevel::Warning,
                message: if detail.is_empty() {
                    format!("systemd unit `{unit}` is not active")
                } else {
                    format!("systemd unit `{unit}` is not active: {detail}")
                },
            }]
        }
        Err(_) => vec![PrecheckIssue {
            code: "SYSTEMD_UNAVAILABLE",
            level: PrecheckLevel::Warning,
            message: format!("systemctl unavailable; cannot verify unit `{unit}` for lifecycle"),
        }],
    }
}

pub(crate) fn ensure_prechecks_pass(checks: &[InstancePrecheck], force: bool) -> StoreResult<()> {
    if checks.iter().all(|check| check.ok_to_register(force)) {
        return Ok(());
    }

    Err(StoreError::PrecheckFailed {
        report: format_precheck_report(checks),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbm_core::Engine;
    use dbm_discovery::{Confidence, DiscoveredInstance, InstanceRunStatus};

    fn sample_item(host: &str, port: u16) -> DiscoveredInstance {
        DiscoveredInstance {
            discovery_id: "dsc_test".into(),
            fingerprint: "sha256:test".into(),
            engine: Engine::Postgres,
            host: host.into(),
            port,
            socket_path: None,
            data_dir: None,
            systemd_unit: None,
            version: None,
            status: InstanceRunStatus::Running,
            sources: Vec::new(),
            confidence: Confidence::Medium,
            already_registered: false,
            registered_instance_id: None,
            scanned_at: "0".into(),
        }
    }

    #[test]
    fn unreachable_host_is_warning() {
        let item = sample_item("invalid..host", 5432);
        let issue = check_reachability(&item).unwrap();
        assert_eq!(issue.code, "UNREACHABLE");
        assert_eq!(issue.level, PrecheckLevel::Warning);
    }

    #[test]
    fn missing_data_dir_is_warning() {
        let issues = check_data_dir(None);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "NO_DATA_DIR");
    }

    #[test]
    fn ok_to_register_respects_force() {
        let check = InstancePrecheck {
            discovery_id: "dsc_x".into(),
            target: "h:1".into(),
            issues: vec![PrecheckIssue {
                code: "UNREACHABLE",
                level: PrecheckLevel::Warning,
                message: "x".into(),
            }],
        };
        assert!(!check.ok_to_register(false));
        assert!(check.ok_to_register(true));
    }

    #[test]
    fn precheck_blocks_register_without_force() {
        let store = super::super::Store::open_in_memory().unwrap();
        seed_discovery_row(&store, "dsc_block", "192.0.2.1", 9);

        let options = RegisterOptions::default();
        let checks = store
            .precheck_register(&["dsc_block".into()], &options)
            .unwrap();
        assert!(!checks[0].ok_to_register(false));

        let err = store
            .register_discovered(&["dsc_block".into()], options)
            .unwrap_err();
        assert!(matches!(err, StoreError::PrecheckFailed { .. }));

        let forced = RegisterOptions {
            force: true,
            ..Default::default()
        };
        let result = store
            .register_discovered(&["dsc_block".into()], forced)
            .unwrap();
        assert_eq!(result.instances.len(), 1);
    }

    fn seed_discovery_row(store: &super::super::Store, discovery_id: &str, host: &str, port: u16) {
        store
            .sqlite()
            .execute(
                "INSERT INTO discovery_scans (id, started_at, completed_at, instance_count)
                 VALUES ('scan_test', datetime('now'), datetime('now'), 1)",
                [],
            )
            .unwrap();
        store
            .sqlite()
            .execute(
                "INSERT INTO discovery_cache (
                    discovery_id, scan_id, fingerprint, engine, host, port,
                    status, sources_json, confidence, already_registered, scanned_at
                 ) VALUES (?1, 'scan_test', ?2, 'postgres', ?3, ?4, 'running', '[]', 'medium', 0, '0')",
                rusqlite::params![
                    discovery_id,
                    format!("sha256:{discovery_id}"),
                    host,
                    i64::from(port),
                ],
            )
            .unwrap();
    }
}
