//! Results feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::ResultsState;
use super::detail::view as detail_view;

pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &ResultsState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // results table (placeholder)
            Constraint::Length(8), // detail pane
        ])
        .split(area);

    frame.render_widget(
        ratatui::widgets::Block::default().title("Results"),
        chunks[0],
    );
    detail_view::render(frame, theme, chunks[1], &state.detail);
}
