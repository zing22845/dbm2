//! Instance workspace feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use super::state::IwState;
use super::connections::view as connections_view;
use super::overview::view as overview_view;

/// Render the instance workspace feature: the overview panel on top and the
/// connections panel below.
pub fn render(frame: &mut Frame, area: Rect, state: &IwState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50), // overview
            Constraint::Percentage(50), // connections
        ])
        .split(area);

    overview_view::render(frame, chunks[0], &state.overview);
    connections_view::render(frame, chunks[1], &state.connections);
}
