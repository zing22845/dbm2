//! Editor feature rendering.
//!
//! Renders the SQL editor buffer (via the shared `render_editor`), the SQL
//! completion popup over it, and the context picker sidebar.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::common::editor;
use crate::common::view::theme::Theme;

use super::state::EditorState;
use super::context_picker::view as cp_view;
use super::sql_completion::view as sc_view;

/// Render the editor feature. Returns the hardware cursor position if the
/// editor is visible (the caller places the terminal cursor).
pub fn render(frame: &mut Frame, _theme: &Theme, area: Rect, state: &EditorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0), // editor body
        ])
        .split(area);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(30), // context picker sidebar
        ])
        .split(chunks[0]);

    let mut editor = state.editor.clone();
    let _cursor = editor::render_editor(&mut editor, body[0], frame.buffer_mut());
    cp_view::render(frame, _theme, body[1], &state.context_picker);
    sc_view::render(frame, _theme, body[0], &state.sql_completion);
}
