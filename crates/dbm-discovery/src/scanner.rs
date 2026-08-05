use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use dbm_core::Engine;

use crate::context::{DiscoveryConfig, is_loopback_host};
use crate::merge::{finalize_instances, merge_candidates};
use crate::providers::{DiscoveryProvider, PortProvider, local_postgres_providers};
use crate::types::ScanResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanProgress {
    pub hosts_done: u32,
    pub hosts_total: u32,
}

#[derive(Clone, Copy)]
pub struct ScanOptions<'a> {
    pub progress: Option<&'a dyn Fn(ScanProgress)>,
    pub cancel: Option<&'a AtomicBool>,
}

pub struct DiscoveryScanner {
    local_providers: Vec<Box<dyn DiscoveryProvider>>,
    port_provider: PortProvider,
}

impl DiscoveryScanner {
    pub fn postgres_default() -> Self {
        Self {
            local_providers: local_postgres_providers(),
            port_provider: PortProvider,
        }
    }

    /// Port-only scanner for deterministic tests (no process/pidfile/socket noise).
    pub fn port_only() -> Self {
        Self {
            local_providers: Vec::new(),
            port_provider: PortProvider,
        }
    }

    pub fn scan(
        &self,
        config: &DiscoveryConfig,
        registered: &HashMap<String, String>,
    ) -> ScanResult {
        self.scan_with_options(
            config,
            registered,
            ScanOptions {
                progress: None,
                cancel: None,
            },
        )
    }

    pub fn scan_with_options(
        &self,
        config: &DiscoveryConfig,
        registered: &HashMap<String, String>,
        opts: ScanOptions<'_>,
    ) -> ScanResult {
        let scan_id = format!("scan_{}", uuid::Uuid::new_v4().simple());
        let scanned_at = chrono_now();

        let mut candidates = Vec::new();
        if config.engine == Engine::Postgres {
            if should_run_local_providers(config) {
                for provider in &self.local_providers {
                    if is_cancelled(opts.cancel) {
                        break;
                    }
                    candidates.extend(provider.discover(config));
                }
            }

            if !is_cancelled(opts.cancel) {
                candidates.extend(self.port_provider.discover_with_options(
                    config,
                    opts.progress,
                    opts.cancel,
                ));
            }
        }

        let merged = merge_candidates(candidates);
        let instances = finalize_instances(merged, &scanned_at, registered);

        ScanResult { scan_id, instances }
    }
}

pub(crate) fn should_run_local_providers(config: &DiscoveryConfig) -> bool {
    config
        .effective_targets()
        .iter()
        .any(|target| is_loopback_host(&target.host))
}

fn is_cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::context::DiscoveryTarget;

    fn config_with_targets(targets: Vec<DiscoveryTarget>) -> DiscoveryConfig {
        DiscoveryConfig {
            targets,
            hosts: vec!["ignored.example.com".into()],
            ports: vec![5432],
            max_duration: std::time::Duration::from_secs(1),
            engine: dbm_core::Engine::Postgres,
        }
    }

    #[test]
    fn empty_scan_returns_result() {
        let scanner = DiscoveryScanner::port_only();
        let config = DiscoveryConfig {
            targets: vec![],
            hosts: vec!["127.0.0.1".into()],
            ports: vec![1],
            max_duration: std::time::Duration::from_secs(1),
            engine: dbm_core::Engine::Postgres,
        };
        let result = scanner.scan(&config, &HashMap::new());
        assert!(result.scan_id.starts_with("scan_"));
        assert!(result.instances.is_empty());
    }

    #[test]
    fn remote_only_skips_local_providers() {
        let config = config_with_targets(vec![DiscoveryTarget {
            host: "192.0.2.1".into(),
            ports: vec![1],
        }]);

        assert!(!should_run_local_providers(&config));
    }

    #[test]
    fn loopback_target_enables_local_providers() {
        let config = config_with_targets(vec![
            DiscoveryTarget {
                host: "192.0.2.1".into(),
                ports: vec![1],
            },
            DiscoveryTarget {
                host: "localhost".into(),
                ports: vec![1],
            },
        ]);

        assert!(should_run_local_providers(&config));
    }

    #[test]
    fn scan_reports_progress_for_each_effective_target() {
        let scanner = DiscoveryScanner::port_only();
        let config = config_with_targets(vec![
            DiscoveryTarget {
                host: "127.0.0.1".into(),
                ports: vec![1],
            },
            DiscoveryTarget {
                host: "::1".into(),
                ports: vec![1],
            },
        ]);
        let progress = Mutex::new(Vec::new());
        let on_progress = |update| progress.lock().unwrap().push(update);

        scanner.scan_with_options(
            &config,
            &HashMap::new(),
            ScanOptions {
                progress: Some(&on_progress),
                cancel: None,
            },
        );

        assert_eq!(
            *progress.lock().unwrap(),
            vec![
                ScanProgress {
                    hosts_done: 1,
                    hosts_total: 2,
                },
                ScanProgress {
                    hosts_done: 2,
                    hosts_total: 2,
                },
            ]
        );
    }

    #[test]
    fn cancellation_after_progress_stops_before_next_host() {
        let scanner = DiscoveryScanner::port_only();
        let config = config_with_targets(vec![
            DiscoveryTarget {
                host: "127.0.0.1".into(),
                ports: vec![1],
            },
            DiscoveryTarget {
                host: "::1".into(),
                ports: vec![1],
            },
        ]);
        let cancel = AtomicBool::new(false);
        let progress = Mutex::new(Vec::new());
        let on_progress = |update| {
            progress.lock().unwrap().push(update);
            cancel.store(true, Ordering::Relaxed);
        };

        scanner.scan_with_options(
            &config,
            &HashMap::new(),
            ScanOptions {
                progress: Some(&on_progress),
                cancel: Some(&cancel),
            },
        );

        assert_eq!(
            *progress.lock().unwrap(),
            vec![ScanProgress {
                hosts_done: 1,
                hosts_total: 2,
            }]
        );
    }

    #[test]
    fn cancelled_scan_stops_before_first_host() {
        let scanner = DiscoveryScanner::port_only();
        let config = config_with_targets(vec![DiscoveryTarget {
            host: "127.0.0.1".into(),
            ports: vec![1],
        }]);
        let cancel = AtomicBool::new(true);
        let progress = Mutex::new(Vec::new());
        let on_progress = |update| progress.lock().unwrap().push(update);

        let result = scanner.scan_with_options(
            &config,
            &HashMap::new(),
            ScanOptions {
                progress: Some(&on_progress),
                cancel: Some(&cancel),
            },
        );

        assert!(result.instances.is_empty());
        assert!(progress.lock().unwrap().is_empty());
    }
}
