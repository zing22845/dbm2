//! Explorer feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use super::state::ExplorerState;
use super::instances::view as instances_view;
use super::objects::view as objects_view;

/// Render the explorer feature: the instances list on top and the objects tree
/// below.
pub fn render(frame: &mut Frame, area: Rect, state: &ExplorerState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40), // instances list
            Constraint::Percentage(60), // objects tree
        ])
        .split(area);

    instances_view::render(frame, chunks[0], &state.instances);
    objects_view::render(frame, chunks[1], &state.objects);
}
