//! Instance overview feature effects and actions.

use std::sync::Arc;

use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;

/// Actions produced by overview effects.
#[derive(Debug, Clone)]
pub enum OverviewAction {
    /// The store returned the instance. Boxed to keep the enum small.
    Loaded {
        instance: Box<dbm_store::ManagedInstance>,
    },
    /// Loading failed.
    Error { error: String },
}

/// Effects emitted by the overview panel.
#[derive(Debug, Clone)]
pub enum OverviewEffect {
    /// Load the managed instance overview by name.
    LoadInstance { instance_name: String },
}

impl Effect for OverviewEffect {
    type Action = OverviewAction;

    fn run(
        self,
        _emit: Emitter<Self::Action>,
        services: Arc<Services>,
    ) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            let store = services.store.clone();
            match self {
                OverviewEffect::LoadInstance { instance_name } => {
                    let result = tokio::task::spawn_blocking(move || {
                        store
                            .lock()
                            .expect("iw store lock")
                            .get_managed_instance_by_name(&instance_name)
                    })
                    .await;
                    match result {
                        Ok(Ok(instance)) => {
                            vec![OverviewAction::Loaded {
                                instance: Box::new(instance),
                            }]
                        }
                        Ok(Err(e)) => vec![OverviewAction::Error {
                            error: e.to_string(),
                        }],
                        Err(e) => vec![OverviewAction::Error {
                            error: e.to_string(),
                        }],
                    }
                }
            }
        })
    }
}
