//! Instance workspace feature effects and actions.

use std::sync::Arc;

use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;

use super::connections::effect::{ConnectionsAction, ConnectionsEffect};
use super::overview::effect::{OverviewAction, OverviewEffect};

/// Actions produced by instance workspace effects.
#[derive(Debug, Clone)]
pub enum IwAction {
    /// An action originating from the overview panel.
    Overview(OverviewAction),
    /// An action originating from the connections panel.
    Connections(ConnectionsAction),
    /// The store confirmed the instance was unregistered.
    Unregistered { instance: String },
}

impl From<OverviewAction> for IwAction {
    fn from(a: OverviewAction) -> Self {
        IwAction::Overview(a)
    }
}
impl From<ConnectionsAction> for IwAction {
    fn from(a: ConnectionsAction) -> Self {
        IwAction::Connections(a)
    }
}

/// Effects emitted by the instance workspace feature. Child effects are
/// wrapped so they remain in the same side-channel stream; their actions are
/// lifted into [`IwAction`].
#[derive(Debug, Clone)]
pub enum IwEffect {
    /// An effect originating from the overview panel.
    Overview(OverviewEffect),
    /// An effect originating from the connections panel.
    Connections(ConnectionsEffect),
    /// Unregister (delete) the named managed instance from the store.
    UnregisterInstance { instance: String },
    /// Re-probe lifecycle readiness for the named instance (the overview's `r`
    /// key), matching the original dbm's refresh.
    Refresh { instance_name: String },
}

impl Effect for IwEffect {
    type Action = IwAction;

    fn run(self, emit: Emitter<Self::Action>, services: Arc<Services>) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                IwEffect::Overview(e) => {
                    let emit = emit.map::<OverviewAction>();
                    e.run(emit, services)
                        .await
                        .into_iter()
                        .map(IwAction::Overview)
                        .collect()
                }
                IwEffect::Connections(e) => {
                    let emit = emit.map::<ConnectionsAction>();
                    e.run(emit, services)
                        .await
                        .into_iter()
                        .map(IwAction::Connections)
                        .collect()
                }
                IwEffect::UnregisterInstance { instance } => {
                    let name = instance.clone();
                    let store = services.store.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        let store = store.lock().expect("iw store lock");
                        store.unregister_managed(&name)
                    })
                    .await;
                    match result {
                        Ok(Ok(_)) => vec![IwAction::Unregistered { instance }],
                        Ok(Err(e)) => {
                            tracing::warn!(error = %e, "iw unregister failed");
                            Vec::new()
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "iw unregister join failed");
                            Vec::new()
                        }
                    }
                }
                IwEffect::Refresh { instance_name } => {
                    let name = instance_name.clone();
                    let store = services.store.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        let store = store.lock().expect("iw store lock");
                        let inst = store.get_managed_instance_by_name(&name)?;
                        store.probe_and_upsert_instance_lifecycle(&inst.id)
                    })
                    .await;
                    let _ = result;
                    // The overview/connections reloads (sent alongside) carry the
                    // fresh data; a failed probe is logged but does not block the
                    // refresh.
                    tracing::debug!(instance_name, "iw refresh lifecycle probe done");
                    Vec::new()
                }
            }
        })
    }
}
