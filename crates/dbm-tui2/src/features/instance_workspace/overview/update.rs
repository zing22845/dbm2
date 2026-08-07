//! Instance overview feature update.

use super::msg::OverviewMessage;
use super::state::OverviewState;
use super::intent::OverviewIntent;
use super::effect::OverviewEffect;

/// Update the overview panel state. Pure by-value transition.
///
/// The returned `bool` is `dirty`: whether the rendered overview changed.
/// `Reload` only re-fetches (the later `Loaded` marks dirty); `Loaded` always
/// sets the instance.
pub fn update(
    msg: OverviewMessage,
    mut state: OverviewState,
) -> (OverviewState, Vec<OverviewIntent>, Vec<OverviewEffect>, bool) {
    match msg {
        OverviewMessage::Load { instance_name } => {
            let changed = state.instance_name != instance_name;
            state.instance_name = instance_name.clone();
            return (
                state,
                Vec::new(),
                vec![OverviewEffect::LoadInstance { instance_name }],
                changed,
            );
        }
        OverviewMessage::Reload => {
            let instance_name = state.instance_name.clone();
            return (
                state,
                Vec::new(),
                vec![OverviewEffect::LoadInstance { instance_name }],
                false,
            );
        }
        OverviewMessage::Loaded { instance } => {
            state.instance = Some(*instance);
            (state, Vec::new(), Vec::new(), true)
        }
    }
}
