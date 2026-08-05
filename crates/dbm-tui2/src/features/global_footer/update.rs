//! Global footer feature update.

use super::msg::FooterMessage;
use super::state::FooterState;
use super::intent::FooterIntent;
use super::effect::FooterEffect;

pub fn update(
    _msg: FooterMessage,
    _state: &mut FooterState,
) -> (FooterState, Vec<FooterIntent>, Vec<FooterEffect>) {
    match _msg {}
}
