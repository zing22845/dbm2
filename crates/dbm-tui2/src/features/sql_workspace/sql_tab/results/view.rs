//! Results feature rendering: composes the `list` and `detail` sub-feature
//! views and handles the horizontal split when detail is open.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::detail::view as detail_view;
use super::list::view as list_view;
use super::state::ResultsState;

/// SPLITTER_WIDTH between list and detail sub-panes.
const SPLITTER_WIDTH: u16 = 1;

/// Render the results feature by composing list + detail sub-views.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ResultsState,
    focused: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = theme.palette();

    if state.detail_open {
        let detail_w = state.detail.pane_width;
        let split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(SPLITTER_WIDTH),
                Constraint::Length(detail_w),
            ])
            .split(area);

        // Draw the list pane (left).
        list_view::render(frame, theme, split[0], &state.list, focused, true);

        // Draw the vertical splitter.
        frame.render_widget(
            Paragraph::new(" ").style(Style::default().bg(p.border)),
            split[1],
        );

        // Draw the detail pane (right).
        let body = state.list.selected_cell().unwrap_or_default();
        let col_name = state.list.selected_column_name().unwrap_or("").to_string();
        let title = format!(
            " [{}] row {}",
            if col_name.is_empty() { "?" } else { &col_name },
            state.list.row + 1
        );
        detail_view::render(frame, theme, split[2], &state.detail, &body, title, state.list.edit.editing, focused);
    } else {
        // No detail: the content area is just the list.
        list_view::render(frame, theme, area, &state.list, focused, false);
    }
}