//! Results feature state.

use super::detail::state::DetailState;

#[derive(Debug, Default, Clone)]
pub struct ResultsState {
    pub detail: DetailState,
}
