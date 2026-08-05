//! Results feature effects and actions.

use crate::app::action::Action;
use crate::app_shell::effect::Effect;
use super::detail::effect::DetailEffect;

#[derive(Debug, Clone)]
pub enum ResultsAction {}

impl From<ResultsAction> for Action {
    fn from(_a: ResultsAction) -> Self {
        match _a {}
    }
}

#[derive(Debug, Clone)]
pub enum ResultsEffect {
    Detail(DetailEffect),
}

impl Effect for ResultsEffect {
    type Action = ResultsAction;

    fn run(self) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
            ResultsEffect::Detail(_) => Vec::new(),
        }
        })
    }
}
