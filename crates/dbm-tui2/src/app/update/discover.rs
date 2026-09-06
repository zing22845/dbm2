//! Discover messages: the scan/register flow and closing the discover
//! parent pane.

use super::UpdateResult;
use super::{box_effect, box_intent, close_discover, explorer_load_instances_msg};
use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::app_shell::nav::DiscoverPane;
use crate::app_shell::pane::Pane;
use crate::features::discover::msg::{DiscoverMessage, DiscoverMsg};
use crate::features::discover::update::update as discover_update;

pub(super) fn apply(msg: AppMsg, state: &mut AppState, result: &mut UpdateResult) {
    let AppMsg::Discover(m) = msg else {
        return;
    };
    // The discover parent pane's child-pane focus lives on `state.focus`,
    // so a focus change (and moving focus to results on scan) is applied
    // here before/with the discover feature's content update.
    let mut discover_dirty = false;
    if let DiscoverMsg::Message(DiscoverMessage::Focus(sub)) = &m {
        state.focus = Pane::Discover(*sub);
        discover_dirty = true;
    }
    if let DiscoverMsg::Message(DiscoverMessage::StartScan) = &m {
        state.focus = Pane::Discover(DiscoverPane::Results);
        discover_dirty = true;
    }
    // Closing the modal is shell orchestration, handled after the
    // discover feature's own update so the frame is ready for teardown.
    let should_close = matches!(&m, DiscoverMsg::Message(DiscoverMessage::Close));
    // A successful register writes to the store while the discover modal
    // stays open; reload the explorer instance tree right away so the
    // newly registered instance appears on the left immediately, without
    // waiting for the modal to close.
    let should_reload_instances = matches!(
        &m,
        DiscoverMsg::Message(DiscoverMessage::RegisterComplete { .. })
    );
    let DiscoverMsg::Message(inner) = m;
    // The discover feature's update is a pure by-value transition: move
    // the state out, update it, move the result back. No deep clone.
    let discover = std::mem::take(&mut state.discover);
    let (s, intents, effects, d) = discover_update(inner, discover);
    state.discover = s;
    if should_close {
        close_discover(state);
        discover_dirty = true;
        // A close also re-fetches the explorer instance tree so any
        // instances registered before closing still show up (a safety
        // net for the immediate reload below).
        result.pending.push_back(explorer_load_instances_msg());
    } else if should_reload_instances {
        // Registering succeeded while the modal is open: reload the
        // explorer instance tree now so the new instance appears on the
        // left immediately (the user does not have to close discover
        // to see it).
        result.pending.push_back(explorer_load_instances_msg());
    }
    result.dirty |= d || discover_dirty;
    result.intents.extend(intents.into_iter().map(box_intent));
    result.effects.extend(effects.into_iter().map(box_effect));
}

#[cfg(test)]
mod tests {
    use super::super::update;
    use super::super::{close_discover, open_discover};
    use super::*;

    fn focus_changed_msg(pane: Pane) -> AppMsg {
        AppMsg::Shell(crate::app_shell::msg::ShellMsg::FocusChanged { pane })
    }

    #[test]
    fn focus_changed_rejected_while_discover_owns_focus() {
        let mut state = AppState::default();
        state.focus = Pane::Discover(DiscoverPane::Engine);
        let before = state.focus;
        let result = update(
            focus_changed_msg(Pane::Explorer(
                crate::app_shell::nav::ExplorerPane::default(),
            )),
            &mut state,
        );
        // Focus stays on discover: the single choke point blocks leaving it.
        assert_eq!(state.focus, before);
        assert!(!result.dirty, "rejected focus change must not mark dirty");
    }
    #[test]
    fn discover_close_returns_to_explorer_preserving_subpane_and_cursor() {
        use crate::app_shell::nav::ExplorerPane;

        let mut state = AppState::default();
        // User works on the explorer's objects sub-pane (cursor moved down).
        state.set_focus(Pane::Explorer(ExplorerPane::Objects));
        state.explorer.instances.cursor = 3;

        // Open discover (matches the header-activation path); the explorer
        // sub-pane and cursor are left untouched.
        open_discover(&mut state);
        assert_eq!(state.focus, Pane::Discover(DiscoverPane::Engine));

        // Closing discover hands focus back to the Explorer (as the original
        // dbm does), preserving the sub-pane and cursor — not resetting to an
        // overview or the header.
        close_discover(&mut state);
        assert_eq!(
            state.focus,
            Pane::Explorer(ExplorerPane::Objects),
            "closing discover returns to the explorer sub-pane the user left"
        );
        assert_eq!(state.explorer.pane, ExplorerPane::Objects);
        assert_eq!(state.explorer.instances.cursor, 3);
    }
    #[test]
    fn discover_close_returns_to_explorer_instances_by_default() {
        use crate::app_shell::nav::ExplorerPane;

        // Even if discover is closed without ever focusing a workspace pane
        // (e.g. right after startup on the header), focus lands on the
        // Explorer's instances sub-pane rather than the SQL workspace.
        let mut state = AppState::default();
        state.focus = Pane::Discover(DiscoverPane::Engine);
        close_discover(&mut state);
        assert_eq!(state.focus, Pane::Explorer(ExplorerPane::Instances));
    }
    #[test]
    fn focus_changed_allowed_when_not_discover() {
        let mut state = AppState::default();
        state.focus = Pane::Header;
        update(focus_changed_msg(Pane::SQLWorkspace), &mut state);
        assert_eq!(state.focus, Pane::SQLWorkspace);
    }
}
