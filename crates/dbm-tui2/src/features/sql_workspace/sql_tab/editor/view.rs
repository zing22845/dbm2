//! Editor feature rendering.
//!
//! Renders the SQL editor buffer (via the shared `render_editor`), the SQL
//! completion popup over it, and the context picker sidebar.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::common::components::search::pane_search_title_line;
use crate::common::editor;
use crate::common::view::hints::{draw_footer, footer_height, sql_pane_footer_text};
use crate::common::view::theme::Theme;

use super::state::EditorState;
use super::context_picker::view as cp_view;
use super::sql_completion::view as sc_view;

/// Render the editor feature. Returns the hardware cursor position if the
/// editor is visible (the caller places the terminal cursor).
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &EditorState) {
    let p = theme.palette();

    // The editor gets a titled border like the results / history panes, with
    // the current edit mode and in-buffer search state surfaced in the title.
    let mode = match state.editor.mode {
        edtui::EditorMode::Insert => "insert",
        edtui::EditorMode::Visual => "visual",
        edtui::EditorMode::Search => "search",
        edtui::EditorMode::Normal => "normal",
    };
    let title = pane_search_title_line(
        &format!(" [E] SQL ({mode})"),
        &state.sql_search.search,
        false,
        true,
        Style::default().fg(p.muted),
        0,
        0,
        None,
        Some(Style::default().fg(p.accent)),
        Some(Style::default().fg(p.accent)),
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border_active));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // The footer hint is sized to its wrapped height so a narrow terminal does
    // not clip it; the editor body gets the remaining space.
    let search_active = state.sql_search.text_input_active();
    let hint = sql_pane_footer_text(search_active, search_active, mode, state.sql_search.has_filter());
    let footer_h = footer_height(&hint, inner.width).min(inner.height.saturating_sub(1));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0), // editor body
            Constraint::Length(footer_h), // editor footer hints
        ])
        .split(inner);

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

    // Editor footer hints from the shared builder (wrapped to the pane width).
    draw_footer(frame, theme, chunks[1], &hint);
}
