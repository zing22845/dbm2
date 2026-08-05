//! Instance overview feature effects and actions.

use crate::app_shell::effect::Effect;

#[derive(Debug, Clone)]
pub enum OverviewAction {}

#[derive(Debug, Clone)]
pub enum OverviewEffect {}

impl Effect for OverviewEffect {
    type Action = OverviewAction;

    fn run(self) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {}
        })
    }
}
