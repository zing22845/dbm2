//! Instance connections feature effects and actions.

use crate::app_shell::effect::Effect;

#[derive(Debug, Clone)]
pub enum ConnectionsAction {}

#[derive(Debug, Clone)]
pub enum ConnectionsEffect {}

impl Effect for ConnectionsEffect {
    type Action = ConnectionsAction;

    fn run(self, _emit: crate::app_shell::effect::effect_trait::Emitter<Self::Action>, _services: std::sync::Arc<crate::common::service::services::Services>) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {}
        })
    }
}
