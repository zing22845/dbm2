//! Performance monitor feature effects and actions.

use crate::app_shell::effect::Effect;

#[derive(Debug, Clone)]
pub enum PerfAction {}

#[derive(Debug, Clone)]
pub enum PerfEffect {}

impl Effect for PerfEffect {
    type Action = PerfAction;

    fn run(self) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {}
        })
    }
}
