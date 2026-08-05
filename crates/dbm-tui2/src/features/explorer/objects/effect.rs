//! Explorer objects feature effects and actions.

use crate::app_shell::effect::Effect;

#[derive(Debug, Clone)]
pub enum ObjectsAction {}

#[derive(Debug, Clone)]
pub enum ObjectsEffect {}

impl Effect for ObjectsEffect {
    type Action = ObjectsAction;

    fn run(self) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {}
        })
    }
}
