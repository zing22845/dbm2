//! Explorer feature effects and actions.

use crate::app_shell::effect::Effect;
use super::instances::effect::InstancesEffect;
use super::objects::effect::ObjectsEffect;

/// Actions produced by explorer effects.
#[derive(Debug, Clone)]
pub enum ExplorerAction {}

/// Effects emitted by the explorer feature. Child effects are wrapped so they
/// remain in the same side-channel stream.
#[derive(Debug, Clone)]
pub enum ExplorerEffect {
    /// An effect originating from the instances list.
    Instances(InstancesEffect),
    /// An effect originating from the objects tree.
    Objects(ObjectsEffect),
}

impl Effect for ExplorerEffect {
    type Action = ExplorerAction;

    fn run(self) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                ExplorerEffect::Instances(_) | ExplorerEffect::Objects(_) => Vec::new(),
            }
        })
    }
}
