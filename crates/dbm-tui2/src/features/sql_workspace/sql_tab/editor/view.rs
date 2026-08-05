//! Editor feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::EditorState;
use super::context_picker::view as cp_view;
use super::sql_completion::view as sc_view;

pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &EditorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // editor body (placeholder)
            Constraint::Length(3), // completion popup
        ])
        .split(area);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(30), // context picker sidebar
        ])
        .split(chunks[0]);

    // Editor body placeholder.
    frame.render_widget(
        ratatui::widgets::Block::default().title("Editor"),
        body[0],
    );
    cp_view::render(frame, theme, body[1], &state.context_picker);
    sc_view::render(frame, theme, chunks[1], &state.sql_completion);
}
