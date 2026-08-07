//! Instance workspace feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::IwState;
use super::connections::view as connections_view;
use super::overview::view as overview_view;

/// Render the instance workspace: the overview panel on top and the
/// connections panel below. `focused` colors the connections border.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &IwState,
    focused: bool,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // overview
            Constraint::Min(0),    // connections
        ])
        .split(area);

    overview_view::render(frame, theme, chunks[0], &state.overview);
    connections_view::render(frame, theme, chunks[1], &state.connections, focused);
}
