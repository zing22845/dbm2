//! Explorer instances feature effects and actions.

use crate::app_shell::effect::Effect;

#[derive(Debug, Clone)]
pub enum InstancesAction {}

#[derive(Debug, Clone)]
pub enum InstancesEffect {}

impl Effect for InstancesEffect {
    type Action = InstancesAction;

    fn run(self) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {}
        })
    }
}
