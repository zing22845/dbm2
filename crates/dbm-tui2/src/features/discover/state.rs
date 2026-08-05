//! Discover feature state.

use super::engine::state::EngineState;
use super::results::state::ResultsState;
use super::targets::state::TargetsState;

/// Which discover pane owns keyboard input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiscoverFocus {
    Engine,
    #[default]
    Targets,
    Results,
}

/// State for the discover feature, aggregating its three child sub-modules.
#[derive(Debug, Clone, Default)]
pub struct DiscoverState {
    /// The discovery engine selector.
    pub engine: EngineState,
    /// The discovery targets editor.
    pub targets: TargetsState,
    /// The discovery results list.
    pub results: ResultsState,
    /// Which discover pane is focused.
    pub focus: DiscoverFocus,
    /// Whether the close-confirmation dialog is shown.
    pub close_confirm: bool,
}

impl DiscoverState {
    /// Open a fresh discover modal with the default loopback target.
    pub fn opened() -> Self {
        DiscoverState {
            targets: TargetsState::with_default_targets(),
            focus: DiscoverFocus::Engine,
            ..DiscoverState::default()
        }
    }
}
