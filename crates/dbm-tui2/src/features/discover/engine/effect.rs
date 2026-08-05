//! Engine selector feature effects and actions.

use crate::app_shell::effect::Effect;

#[derive(Debug, Clone)]
pub enum EngineAction {}

#[derive(Debug, Clone)]
pub enum EngineEffect {}

impl Effect for EngineEffect {
    type Action = EngineAction;

    fn run(self) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {}
        })
    }
}
