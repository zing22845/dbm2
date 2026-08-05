//! Global footer feature effects and actions.

use crate::app_shell::effect::Effect;

#[derive(Debug, Clone)]
pub enum FooterAction {}

#[derive(Debug, Clone)]
pub enum FooterEffect {}

impl Effect for FooterEffect {
    type Action = FooterAction;

    fn run(self) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {}
        })
    }
}
