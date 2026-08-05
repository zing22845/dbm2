//! Context picker sub-module effects and actions.

use crate::app::action::Action;
use crate::app_shell::effect::Effect;

#[derive(Debug, Clone)]
pub enum ContextPickerAction {}

impl From<ContextPickerAction> for Action {
    fn from(_a: ContextPickerAction) -> Self {
        match _a {}
    }
}

#[derive(Debug, Clone)]
pub enum ContextPickerEffect {}

impl Effect for ContextPickerEffect {
    type Action = ContextPickerAction;

    fn run(self, _emit: crate::app_shell::effect::effect_trait::Emitter<Self::Action>, _services: std::sync::Arc<crate::common::service::services::Services>) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {}
        })
    }
}
