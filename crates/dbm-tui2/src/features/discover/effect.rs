//! Discover feature effects and actions.

use crate::app_shell::effect::Effect;
use super::engine::effect::EngineEffect;
use super::results::effect::ResultsEffect;
use super::targets::effect::TargetsEffect;

/// Actions produced by discover effects.
#[derive(Debug, Clone)]
pub enum DiscoverAction {}

/// Effects emitted by the discover feature. Child effects are wrapped so they
/// remain in the same side-channel stream.
#[derive(Debug, Clone)]
pub enum DiscoverEffect {
    /// An effect originating from the engine selector.
    Engine(EngineEffect),
    /// An effect originating from the targets editor.
    Targets(TargetsEffect),
    /// An effect originating from the results list.
    Results(ResultsEffect),
}

impl Effect for DiscoverEffect {
    type Action = DiscoverAction;

    fn run(self) -> crate::app_shell::effect::effect_trait::BoxFuture<Vec<Self::Action>> {
        Box::pin(async move {
            match self {
                DiscoverEffect::Engine(_)
                | DiscoverEffect::Targets(_)
                | DiscoverEffect::Results(_) => Vec::new(),
            }
        })
    }
}
