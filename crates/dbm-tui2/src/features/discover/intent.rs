//! Discover feature intents.

use crate::app_shell::intent::Intent;
use super::msg::DiscoverMsg;
use super::engine::intent::EngineIntent;
use super::results::intent::ResultsIntent;
use super::targets::intent::TargetsIntent;

/// Intents emitted by the discover feature. Child intents are wrapped so they
/// can be lifted into the global router via `DiscoverMsg`.
#[derive(Debug, Clone)]
pub enum DiscoverIntent {
    /// An intent originating from the engine selector.
    Engine(EngineIntent),
    /// An intent originating from the targets editor.
    Targets(TargetsIntent),
    /// An intent originating from the results list.
    Results(ResultsIntent),
}

impl Intent for DiscoverIntent {
    type Message = DiscoverMsg;

    // The child intent enums (`EngineIntent`/`TargetsIntent`/`ResultsIntent`)
    // are currently uninhabited, so every arm below is unreachable and the
    // match is an empty branch. The child `*Msg` enums already carry variants;
    // remove this allow when real business intents are introduced.
    #[allow(unreachable_code)]
    fn into_message(self) -> Self::Message {
        match self {
            DiscoverIntent::Engine(i) => i.into_message().into(),
            DiscoverIntent::Targets(i) => i.into_message().into(),
            DiscoverIntent::Results(i) => i.into_message().into(),
        }
    }
}
