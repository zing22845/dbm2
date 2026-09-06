//! Shell chrome messages: modal open/close, the explorer width and the
//! header / footer / perf panes.

use super::UpdateResult;
use super::{box_effect, box_intent, open_discover};
use crate::app::msg::AppMsg;
use crate::app::state::AppState;
use crate::features::global_footer::msg::FooterMsg;
use crate::features::global_footer::update::update as footer_update;
use crate::features::header::msg::{HeaderMessage, HeaderMsg};
use crate::features::header::update::update as header_update;
use crate::features::perf_monitor::msg::PerfMsg;
use crate::features::perf_monitor::update::update as perf_update;

pub(super) fn apply(msg: AppMsg, state: &mut AppState, result: &mut UpdateResult) {
    match msg {
        AppMsg::OpenModal(modal) => {
            state.modal = Some(modal);
            result.dirty = true;
        }
        AppMsg::CloseModal => {
            state.modal = None;
            result.dirty = true;
        }
        AppMsg::SetExplorerWidth(width) => {
            // Only repaint when the split actually moved — a nudge/drag that does
            // not change the width (e.g. at a boundary) must not count as a
            // redundant redraw and inflate the waste metric.
            result.dirty = state.splitter.set_explorer_pane_width(width);
        }
        AppMsg::Header(m) => {
            // Opening a modal is shell orchestration, handled before the
            // header feature's own update so the modal state is ready for the
            // frame that follows.
            let opened_discover = if let HeaderMsg::Message(HeaderMessage::Activate) = &m
                && state.header.button == 0
            {
                open_discover(state);
                true
            } else {
                false
            };
            let HeaderMsg::Message(inner) = m;
            // The header feature's update is a pure by-value transition: move
            // the state out, update it, move the result back. No deep clone.
            let header = std::mem::take(&mut state.header);
            let (s, intents, effects, d) = header_update(inner, header);
            state.header = s;
            result.dirty |= d || opened_discover;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Footer(m) => {
            let FooterMsg::Message(inner) = m;
            // The footer feature's update is a pure by-value transition: move
            // the state out, update it, move the result back. No deep clone.
            let footer = std::mem::take(&mut state.footer);
            let (s, intents, effects, d) = footer_update(inner, footer);
            state.footer = s;
            // Keep the shell-level `global_status` mirror in sync with the
            // footer's authoritative status, so other code reading
            // `AppState::global_status` sees the latest value.
            state.global_status = state.footer.status.clone();
            result.dirty |= d;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        AppMsg::Perf(m) => {
            let PerfMsg::Message(inner) = m;
            let (s, intents, effects, d) = perf_update(inner, &mut state.perf);
            state.perf = s;
            result.dirty |= d;
            result.intents.extend(intents.into_iter().map(box_intent));
            result.effects.extend(effects.into_iter().map(box_effect));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::update;
    use super::*;

    #[test]
    fn set_explorer_width_updates_and_clamps_the_splitter() {
        let mut state = AppState::default();
        // `SetExplorerWidth` bypasses the focus guard (app-level state).
        update(AppMsg::SetExplorerWidth(40), &mut state);
        assert_eq!(state.splitter.explorer_pane_width, 40);
        // Out-of-range values are clamped on the way in.
        update(AppMsg::SetExplorerWidth(9999), &mut state);
        assert_eq!(
            state.splitter.explorer_pane_width,
            crate::features::app_splitter::state::MAX_EXPLORER_WIDTH
        );
        update(AppMsg::SetExplorerWidth(0), &mut state);
        assert_eq!(
            state.splitter.explorer_pane_width,
            crate::features::app_splitter::state::MIN_EXPLORER_WIDTH
        );
    }
}
