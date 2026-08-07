//! Discover feature effects and actions.
//!
//! The scan and register effects are the discover feature's only side effects.
//! They run the (synchronous, blocking) `dbm-store` calls on a blocking task and
//! stream progress back through the effect emitter.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dbm_discovery::{DiscoveryConfig, ScanOptions, ScanProgress};
use dbm_store::RunDiscoveryOptions;

use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;

/// Actions produced by discover effects.
#[derive(Debug, Clone)]
pub enum DiscoverAction {
    /// Streamed scan progress update.
    ScanProgress { done: u32, total: u32 },
    /// The scan completed, producing the discovered instances.
    ScanComplete { items: Vec<dbm_discovery::DiscoveredInstance> },
    /// The scan was cancelled before completing.
    ScanCancelled,
    /// The scan failed.
    ScanError { error: String },
    /// A batch of instances was registered.
    RegisterComplete { count: usize },
    /// Registering failed.
    RegisterError { error: String },
}

/// Effects emitted by the discover feature.
#[derive(Debug, Clone)]
pub enum DiscoverEffect {
    /// Run a discovery scan over the given config. `cancel` is the shared flag
    /// the user can set via `CancelScan` (`c`) to stop the scan at the next
    /// host boundary. The results list is read back as the full set; the
    /// unregistered-only filter is applied in the UI layer.
    StartScan { config: DiscoveryConfig, cancel: Arc<AtomicBool> },
    /// Ask an in-flight scan (identified by its shared flag) to stop.
    CancelScan { cancel: Arc<AtomicBool> },
    /// Register the discovered instances with the given discovery ids.
    /// `force` bypasses precheck warnings (the `R` key); errors still block.
    RegisterInstances { discovery_ids: Vec<String>, force: bool },
}

impl Effect for DiscoverEffect {
    type Action = DiscoverAction;

    fn run(self, emit: Emitter<Self::Action>, services: Arc<Services>) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                DiscoverEffect::StartScan { config, cancel } => {
                    run_scan(config, cancel, emit, services).await
                }
                DiscoverEffect::CancelScan { cancel } => {
                    cancel.store(true, Ordering::Relaxed);
                    Vec::new()
                }
                DiscoverEffect::RegisterInstances { discovery_ids, force } => {
                    run_register(discovery_ids, force, emit, services).await
                }
            }
        })
    }
}

async fn run_scan(
    config: DiscoveryConfig,
    cancel: Arc<AtomicBool>,
    emit: Emitter<DiscoverAction>,
    services: Arc<Services>,
) -> Vec<DiscoverAction> {
    let store = services.store.clone();
    let emit_progress = emit.clone();
    // The blocking task uses its own clone; the original `cancel` is kept here
    // to read the cancellation state once the scan returns.
    let task_cancel = Arc::clone(&cancel);
    let scan = tokio::task::spawn_blocking(move || {
        let store = store.lock().expect("discover store lock");
        let scan_options = ScanOptions {
            progress: Some(&|p: ScanProgress| {
                emit_progress.emit(DiscoverAction::ScanProgress {
                    done: p.hosts_done,
                    total: p.hosts_total,
                });
            }),
            cancel: Some(&task_cancel),
        };
        store.run_discovery_with_scan_options(
            config,
            RunDiscoveryOptions {
                include_managed_hosts: false,
            },
            scan_options,
        )
    })
    .await;

    // A cancelled scan yields no new results: report it as cancelled rather
    // than listing whatever was scanned so far.
    if cancel.load(Ordering::Relaxed) {
        return vec![DiscoverAction::ScanCancelled];
    }

    let scan = match scan {
        Ok(result) => result,
        Err(e) => return vec![DiscoverAction::ScanError { error: e.to_string() }],
    };
    if let Err(e) = scan {
        return vec![DiscoverAction::ScanError { error: e.to_string() }];
    }

    // The scan was persisted by the store; read the fresh cache back as the
    // full list. The unregistered-only filter is applied in the UI layer
    // (`ResultsState::visible_indices`) so that toggling `u` can show or hide
    // already-registered instances without re-querying the store.
    let store = services.store.clone();
    let listed = tokio::task::spawn_blocking(move || {
        let store = store.lock().expect("discover store lock");
        store.list_discovered(false)
    })
    .await;

    match listed {
        Ok(Ok(items)) => vec![DiscoverAction::ScanComplete { items }],
        Ok(Err(e)) => vec![DiscoverAction::ScanError { error: e.to_string() }],
        Err(e) => vec![DiscoverAction::ScanError { error: e.to_string() }],
    }
}

async fn run_register(
    discovery_ids: Vec<String>,
    force: bool,
    _emit: Emitter<DiscoverAction>,
    services: Arc<Services>,
) -> Vec<DiscoverAction> {
    tracing::debug!(
        force,
        count = discovery_ids.len(),
        discovery_ids = ?discovery_ids,
        "run_register: calling store.register_discovered"
    );
    let store = services.store.clone();
    let result = tokio::task::spawn_blocking(move || {
        let store = store.lock().expect("discover store lock");
        store.register_discovered(
            &discovery_ids,
            dbm_store::RegisterOptions { name: None, force },
        )
    })
    .await;

    match result {
        Ok(Ok(reg)) => {
            tracing::debug!(
                count = reg.instances.len(),
                "run_register: register_discovered succeeded"
            );
            vec![DiscoverAction::RegisterComplete {
                count: reg.instances.len(),
            }]
        }
        Ok(Err(e)) => {
            // A plain register (`r`) stops on precheck warnings; tell the user
            // they can force-register (`R`) to bypass warnings (errors still
            // block). Force-register failures are reported as-is.
            tracing::warn!(error = %e, "run_register: register_discovered failed");
            let error = if !force && matches!(e, dbm_store::StoreError::PrecheckFailed { .. }) {
                format!("{e}\nuse R to force register (bypass warnings)")
            } else {
                e.to_string()
            };
            vec![DiscoverAction::RegisterError { error }]
        }
        Err(e) => {
            tracing::warn!(error = %e, "run_register: spawn_blocking join failed");
            vec![DiscoverAction::RegisterError { error: e.to_string() }]
        }
    }
}
