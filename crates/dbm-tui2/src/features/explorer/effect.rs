//! Explorer feature effects and actions.

use std::sync::Arc;

use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;

use super::instances::effect::{InstancesAction, InstancesEffect};
use super::objects::effect::{ObjectsAction, ObjectsEffect};

/// Actions produced by explorer effects.
#[derive(Debug, Clone)]
pub enum ExplorerAction {
    /// An action originating from the instances sub-module.
    Instances(InstancesAction),
    /// An action originating from the objects sub-module.
    Objects(ObjectsAction),
}

impl From<InstancesAction> for ExplorerAction {
    fn from(a: InstancesAction) -> Self {
        ExplorerAction::Instances(a)
    }
}

impl From<ObjectsAction> for ExplorerAction {
    fn from(a: ObjectsAction) -> Self {
        ExplorerAction::Objects(a)
    }
}

/// Effects emitted by the explorer feature. Child effects are wrapped so they
/// remain in the same side-channel stream; their actions are lifted into
/// [`ExplorerAction`].
#[derive(Debug, Clone)]
pub enum ExplorerEffect {
    /// An effect originating from the instances list.
    Instances(InstancesEffect),
    /// An effect originating from the objects tree.
    Objects(ObjectsEffect),
}

impl Effect for ExplorerEffect {
    type Action = ExplorerAction;

    fn run(self, emit: Emitter<Self::Action>, services: Arc<Services>) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                ExplorerEffect::Instances(e) => {
                    let emit = emit.map::<InstancesAction>();
                    e.run(emit, services)
                        .await
                        .into_iter()
                        .map(ExplorerAction::Instances)
                        .collect()
                }
                ExplorerEffect::Objects(e) => {
                    let emit = emit.map::<ObjectsAction>();
                    e.run(emit, services)
                        .await
                        .into_iter()
                        .map(ExplorerAction::Objects)
                        .collect()
                }
            }
        })
    }
}
