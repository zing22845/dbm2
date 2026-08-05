//! Explorer feature rendering.

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::{ExplorerPane, ExplorerState};
use super::instances::view as instances_view;
use super::objects::view as objects_view;

/// Render the explorer feature: the active pane (instances tree today).
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &ExplorerState) {
    match state.pane {
        ExplorerPane::Instances => instances_view::render(frame, theme, area, &state.instances),
        ExplorerPane::Objects => objects_view::render(frame, theme, area, &state.objects),
    }
}
