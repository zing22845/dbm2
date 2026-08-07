//! Explorer instances feature effects and actions.

use std::sync::Arc;

use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;

/// Actions produced by instances effects.
#[derive(Debug, Clone)]
pub enum InstancesAction {
    /// The managed instances were loaded from the store.
    InstancesLoaded { instances: Vec<dbm_store::ManagedInstance> },
    /// A specific instance's connections were loaded.
    ConnectionsLoaded {
        instance_idx: usize,
        connections: Vec<dbm_store::InstanceConnection>,
    },
    /// Loading failed.
    LoadError { error: String },
}

/// Effects emitted by the instances feature.
#[derive(Debug, Clone)]
pub enum InstancesEffect {
    /// Load all managed instances from the store.
    LoadInstances,
    /// Load a specific instance's connections from the store.
    LoadConnections { instance_idx: usize, instance_name: String },
}

impl Effect for InstancesEffect {
    type Action = InstancesAction;

    fn run(self, _emit: Emitter<Self::Action>, services: Arc<Services>) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                InstancesEffect::LoadInstances => load_instances(services).await,
                InstancesEffect::LoadConnections { instance_idx, instance_name } => {
                    load_connections(instance_idx, instance_name, services).await
                }
            }
        })
    }
}

async fn load_instances(services: Arc<Services>) -> Vec<InstancesAction> {
    let store = services.store.clone();
    let result = tokio::task::spawn_blocking(move || {
        store.lock().expect("explorer store lock").list_managed_instances()
    })
    .await;
    match result {
        Ok(Ok(instances)) => {
            tracing::debug!(
                count = instances.len(),
                names = ?instances.iter().map(|i| &i.name).collect::<Vec<_>>(),
                "explorer: load_instances succeeded"
            );
            vec![InstancesAction::InstancesLoaded { instances }]
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "explorer: load_instances failed");
            vec![InstancesAction::LoadError { error: e.to_string() }]
        }
        Err(e) => {
            tracing::warn!(error = %e, "explorer: load_instances join failed");
            vec![InstancesAction::LoadError { error: e.to_string() }]
        }
    }
}

async fn load_connections(
    instance_idx: usize,
    instance_name: String,
    services: Arc<Services>,
) -> Vec<InstancesAction> {
    let store = services.store.clone();
    let result = tokio::task::spawn_blocking(move || {
        store
            .lock()
            .expect("explorer store lock")
            .list_instance_connections(&instance_name)
    })
    .await;
    match result {
        Ok(Ok(connections)) => vec![InstancesAction::ConnectionsLoaded {
            instance_idx,
            connections,
        }],
        Ok(Err(e)) => vec![InstancesAction::LoadError { error: e.to_string() }],
        Err(e) => vec![InstancesAction::LoadError { error: e.to_string() }],
    }
}
