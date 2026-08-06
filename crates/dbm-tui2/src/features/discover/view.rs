//! Discover feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::DiscoverState;
use super::engine::view as engine_view;
use super::results::view as results_view;
use super::targets::view as targets_view;

/// Render the discover feature: engine selector on top, with the targets editor
/// and results list stacked vertically below (mirrors the original dbm layout,
/// where targets sits above results and they are separated by a splitter row).
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &DiscoverState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // engine selector
            Constraint::Min(0),    // targets + results
        ])
        .split(area);

    engine_view::render(frame, theme, chunks[0], &state.engine);

    // Stacked vertically, like the original: targets (top), a 1-row splitter,
    // then results (bottom). A vertical split means both panes are the same
    // width; the splitter keeps the horizontal divider visible.
    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50), // targets editor
            Constraint::Length(1),      // splitter
            Constraint::Percentage(50), // results list
        ])
        .split(chunks[1]);

    targets_view::render(frame, theme, body[0], &state.targets);
    crate::common::view::splitter::draw(
        frame,
        body[1],
        crate::common::view::splitter::SplitOrientation::Horizontal,
        false,
        false,
    );
    results_view::render(frame, theme, body[2], &state.results);

    // The close-confirmation dialog is intentionally a placeholder overlay for
    // now; its interactive confirm/cancel is wired once the modal teardown is
    // implemented in the shell.
    let _ = state.close_confirm;
}
