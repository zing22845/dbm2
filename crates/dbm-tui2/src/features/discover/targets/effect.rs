//! Discovery targets editor feature effects and actions.

use crate::app_shell::effect::Effect;

#[derive(Debug, Clone)]
pub enum TargetsAction {}

#[derive(Debug, Clone)]
pub enum TargetsEffect {}

impl Effect for TargetsEffect {
    type Action = TargetsAction;

    fn run(self) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {}
        })
    }
}
