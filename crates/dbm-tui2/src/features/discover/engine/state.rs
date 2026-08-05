//! Engine selector feature state.

use dbm_core::Engine;

/// State for the discovery engine selector.
#[derive(Debug, Clone)]
pub struct EngineState {
    /// The currently selected discovery engine.
    pub engine: Engine,
}

impl Default for EngineState {
    fn default() -> Self {
        EngineState {
            engine: Engine::Postgres,
        }
    }
}
