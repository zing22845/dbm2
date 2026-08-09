//! Engine selector feature state.

use dbm_core::Engine;

/// State for the discovery engine selector.
#[derive(Debug, Clone)]
pub struct EngineState {
    /// The currently selected discovery engine.
    pub engine: Engine,
    /// One-line feedback shown in the engine footer (e.g. the note that only
    /// Postgres is available). Cleared on a fresh discover session.
    pub status: Option<String>,
}

impl Default for EngineState {
    fn default() -> Self {
        EngineState {
            engine: Engine::Postgres,
            status: None,
        }
    }
}
