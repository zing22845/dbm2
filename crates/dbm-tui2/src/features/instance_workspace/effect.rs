//! Instance workspace feature effects and actions.

use crate::app_shell::effect::Effect;
use super::connections::effect::ConnectionsEffect;
use super::overview::effect::OverviewEffect;

/// Actions produced by instance workspace effects.
#[derive(Debug, Clone)]
pub enum IwAction {}

/// Effects emitted by the instance workspace feature. Child effects are
/// wrapped so they remain in the same side-channel stream.
#[derive(Debug, Clone)]
pub enum IwEffect {
    /// An effect originating from the overview panel.
    Overview(OverviewEffect),
    /// An effect originating from the connections panel.
    Connections(ConnectionsEffect),
}

impl Effect for IwEffect {
    type Action = IwAction;

    fn run(self) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                IwEffect::Overview(_) | IwEffect::Connections(_) => Vec::new(),
            }
        })
    }
}
