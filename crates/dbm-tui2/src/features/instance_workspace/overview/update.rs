//! Instance overview feature update.

use super::msg::OverviewMessage;
use super::state::OverviewState;
use super::intent::OverviewIntent;
use super::effect::OverviewEffect;

pub fn update(
    _msg: OverviewMessage,
    _state: OverviewState,
) -> (OverviewState, Vec<OverviewIntent>, Vec<OverviewEffect>) {
    match _msg {}
}
