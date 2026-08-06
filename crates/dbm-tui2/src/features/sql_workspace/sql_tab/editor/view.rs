//! Editor feature rendering.
//!
//! Renders the SQL editor buffer (via the shared `render_editor`), the SQL
//! completion popup over it, and the context picker sidebar.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::common::editor;
use crate::common::view::hints::sql_pane_footer_text;
use crate::common::view::theme::Theme;

use super::state::EditorState;
use super::context_picker::view as cp_view;
use super::sql_completion::view as sc_view;

/// Render the editor feature. Returns the hardware cursor position if the
/// editor is visible (the caller places the terminal cursor).
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &EditorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0), // editor body
            Constraint::Length(1), // editor footer hints
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
    cp_view::render(frame, theme, body[1], &state.context_picker);
    sc_view::render(frame, theme, body[0], &state.sql_completion);

    // Editor footer hints from the shared builder.
    let search_active = state.sql_search.text_input_active();
    let mode = match state.editor.mode {
        edtui::EditorMode::Insert => "insert",
        edtui::EditorMode::Visual => "visual",
        edtui::EditorMode::Search => "search",
        edtui::EditorMode::Normal => "normal",
    };
    let hint = sql_pane_footer_text(search_active, search_active, mode, state.sql_search.has_filter());
    frame.render_widget(
        Paragraph::new(Line::from(hint)).style(Style::default().fg(theme.palette().muted)),
        chunks[1],
    );
}
