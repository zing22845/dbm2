//! Results feature rendering: composes the `list`, `detail`, and `splitter`
//! sub-feature views.
//!
//! A single outer Block with border + title wraps everything. When the detail
//! is visible the Block's inner area splits horizontally into
//! `[list | splitter | detail]`; otherwise the list fills the whole inner area.

use ratatui::style::Style;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::common::components::search::pane_search_title_line;
use crate::common::view::theme::Theme;

use super::detail::view as detail_view;
use super::list::view as list_view;
use super::splitter::view as splitter_view;
use super::state::ResultsState;

/// Render the Results feature. The single outer Block + title wraps the
/// entire `area` (both list and, when `detail_open`, the detail preview).
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: ratatui::layout::Rect,
    state: &ResultsState,
    focused: bool,
    splitter_hover: bool,
    splitter_drag: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = theme.palette();

    let row_count = state.list.row_count();
    let title = pane_search_title_line(
        " [R] Results",
        &state.list.search,
        true,
        false,
        Style::default().fg(p.muted),
        state.list.row,
        row_count,
        None,
        None,
        Some(Style::default().fg(if focused { p.accent } else { p.muted })),
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (list_inner, splitter_rect, detail_inner) = splitter_view::split_inner(
        inner,
        state.detail_open,
        state.splitter.detail_pane_width,
    );

    list_view::render(frame, theme, list_inner, &state.list, focused, state.detail_open);

    if let Some(rect) = splitter_rect {
        splitter_view::render(frame, rect, splitter_hover, splitter_drag);
    }

    if let (Some(detail_area), true) = (detail_inner, state.detail_open) {
        let body = state.list.selected_cell().unwrap_or_default();
        let col_name = state.list.selected_column_name().unwrap_or("").to_string();
        let title_text = format!(
            " [{}] row {}",
            if col_name.is_empty() { "?" } else { &col_name },
            state.list.row + 1
        );
        let edit_editing = state.list.edit.editing;
        detail_view::render(
            frame,
            theme,
            detail_area,
            &state.detail,
            &body,
            title_text,
            edit_editing,
            focused,
        );
    }
}
