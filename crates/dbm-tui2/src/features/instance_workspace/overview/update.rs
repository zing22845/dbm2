//! Instance overview feature update.

use super::msg::OverviewMessage;
use super::state::OverviewState;
use super::intent::OverviewIntent;
use super::effect::OverviewEffect;

/// Update the overview panel state. Pure by-value transition.
pub fn update(
    msg: OverviewMessage,
    mut state: OverviewState,
) -> (OverviewState, Vec<OverviewIntent>, Vec<OverviewEffect>) {
    match msg {
        OverviewMessage::Load { instance_name } => {
            state.instance_name = instance_name.clone();
            return (
                state,
                Vec::new(),
                vec![OverviewEffect::LoadInstance { instance_name }],
            );
        }
        OverviewMessage::Reload => {
            let instance_name = state.instance_name.clone();
            return (
                state,
                Vec::new(),
                vec![OverviewEffect::LoadInstance { instance_name }],
            );
        }
        OverviewMessage::Loaded { instance } => state.instance = Some(*instance),
    }
    (state, Vec::new(), Vec::new())
}
