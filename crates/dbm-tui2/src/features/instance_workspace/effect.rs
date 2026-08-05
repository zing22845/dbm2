//! Instance workspace feature effects and actions.

use std::sync::Arc;

use crate::app_shell::effect::effect_trait::{BoxFuture, Effect, Emitter};
use crate::common::service::services::Services;

use super::connections::effect::{ConnectionsAction, ConnectionsEffect};
use super::overview::effect::{OverviewAction, OverviewEffect};

/// Actions produced by instance workspace effects.
#[derive(Debug, Clone)]
pub enum IwAction {
    /// An action originating from the overview panel.
    Overview(OverviewAction),
    /// An action originating from the connections panel.
    Connections(ConnectionsAction),
}

impl From<OverviewAction> for IwAction {
    fn from(a: OverviewAction) -> Self {
        IwAction::Overview(a)
    }
}
impl From<ConnectionsAction> for IwAction {
    fn from(a: ConnectionsAction) -> Self {
        IwAction::Connections(a)
    }
}

/// Effects emitted by the instance workspace feature. Child effects are
/// wrapped so they remain in the same side-channel stream; their actions are
/// lifted into [`IwAction`].
#[derive(Debug, Clone)]
pub enum IwEffect {
    /// An effect originating from the overview panel.
    Overview(OverviewEffect),
    /// An effect originating from the connections panel.
    Connections(ConnectionsEffect),
}

impl Effect for IwEffect {
    type Action = IwAction;

    fn run(self, emit: Emitter<Self::Action>, services: Arc<Services>) -> BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                IwEffect::Overview(e) => {
                    let emit = emit.map::<OverviewAction>();
                    e.run(emit, services)
                        .await
                        .into_iter()
                        .map(IwAction::Overview)
                        .collect()
                }
                IwEffect::Connections(e) => {
                    let emit = emit.map::<ConnectionsAction>();
                    e.run(emit, services)
                        .await
                        .into_iter()
                        .map(IwAction::Connections)
                        .collect()
                }
            }
        })
    }
}
