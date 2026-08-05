//! Header feature effects and actions.

use crate::app_shell::effect::Effect;

/// Actions produced by header effects.
#[derive(Debug, Clone)]
pub enum HeaderAction {}

/// Effects emitted by the header feature. Empty in the skeleton.
#[derive(Debug, Clone)]
pub enum HeaderEffect {}

impl Effect for HeaderEffect {
    type Action = HeaderAction;

    fn run(self) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {}
        })
    }
}
