//! Discover feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::DiscoverState;
use super::engine::view as engine_view;
use super::results::view as results_view;
use super::targets::view as targets_view;

/// Render the discover feature: engine selector on top, targets editor and
/// results list below.
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &DiscoverState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // engine selector
            Constraint::Min(0),    // targets + results
        ])
        .split(area);

    engine_view::render(frame, theme, chunks[0], &state.engine);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // targets editor
            Constraint::Percentage(50), // results list
        ])
        .split(chunks[1]);

    targets_view::render(frame, theme, body[0], &state.targets);
    results_view::render(frame, theme, body[1], &state.results);

    // The close-confirmation dialog is intentionally a placeholder overlay for
    // now; its interactive confirm/cancel is wired once the modal teardown is
    // implemented in the shell.
    let _ = state.close_confirm;
}
