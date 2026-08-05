//! Discover feature state.

use super::engine::state::EngineState;
use super::results::state::ResultsState;
use super::targets::state::TargetsState;

/// State for the discover feature, aggregating its three child sub-modules.
#[derive(Debug, Default, Clone)]
pub struct DiscoverState {
    /// The discovery engine selector.
    pub engine: EngineState,
    /// The discovery targets editor.
    pub targets: TargetsState,
    /// The discovery results list.
    pub results: ResultsState,
}
