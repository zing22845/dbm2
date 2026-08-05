//! Discover feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use super::state::DiscoverState;
use super::engine::view as engine_view;
use super::results::view as results_view;
use super::targets::view as targets_view;

/// Render the discover feature: engine selector on top, targets editor and
/// results list below.
pub fn render(frame: &mut Frame, area: Rect, state: &DiscoverState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // engine selector
            Constraint::Min(0),    // targets + results
        ])
        .split(area);

    engine_view::render(frame, chunks[0], &state.engine);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // targets editor
            Constraint::Percentage(50), // results list
        ])
        .split(chunks[1]);

    targets_view::render(frame, body[0], &state.targets);
    results_view::render(frame, body[1], &state.results);
}
