//! Discovery results feature effects and actions.

use crate::app_shell::effect::Effect;

#[derive(Debug, Clone)]
pub enum ResultsAction {}

#[derive(Debug, Clone)]
pub enum ResultsEffect {}

impl Effect for ResultsEffect {
    type Action = ResultsAction;

    fn run(self) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {}
        })
    }
}
