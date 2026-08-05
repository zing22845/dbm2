//! Results detail sub-module effects and actions.

use crate::app::action::Action;
use crate::app_shell::effect::Effect;

#[derive(Debug, Clone)]
pub enum DetailAction {}

impl From<DetailAction> for Action {
    fn from(_a: DetailAction) -> Self {
        match _a {}
    }
}

#[derive(Debug, Clone)]
pub enum DetailEffect {}

impl Effect for DetailEffect {
    type Action = DetailAction;

    fn run(self) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {}
        })
    }
}
