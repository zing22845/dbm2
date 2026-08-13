//! History feature rendering: the history list pane with `/` search title and
//! the detail preview.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::components::search::pane_search_title_line;
use crate::common::view::hints::{draw_footer, footer_height, history_list_footer_text};
use crate::common::view::pane_scrollbar::{draw_vertical_pane_scrollbar, pane_scroll_layout};
use crate::common::view::theme::Theme;

use super::state::HistoryState;
use super::store::history_one_line;

/// Render the history list pane for a tab's connection. `focused` drives the
/// border/title highlight so only the current SQL sub-pane is emphasized.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &HistoryState,
    instance: &str,
    connection: &str,
    focused: bool,
) {
    let search_active = state.search.text_input_active();
    let hint = history_list_footer_text(search_active, state.search.has_filter(), true);
    // The footer lives inside the pane's border (like results), so the block
    // wraps the whole area and the inner rect is split into list + footer.
    let footer_h = footer_height(&hint, area.width.saturating_sub(2)).min(area.height.saturating_sub(3));

    let p = theme.palette();
    let entries = state.store.entries(instance, connection);
    let visible = state.visible_indices_for(entries);
    let cursor = state.cursor.min(visible.len().saturating_sub(1));

    let title = pane_search_title_line(
        " [H] History",
        &state.search,
        true,
        false,
        Style::default().fg(p.muted),
        cursor,
        visible.len(),
        Some(area.width.saturating_sub(6)),
        None,
        Some(Style::default().fg(if focused { p.accent } else { p.muted })),
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split the bordered inner area into the list (top) and the footer (bottom).
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1), // history list
            Constraint::Length(footer_h), // footer hints (inside the border)
        ])
        .split(inner);
    let list_area = chunks[0];

    if visible.is_empty() {
        let hint = if state.search.has_filter() {
            "No matching history"
        } else {
            "No history yet"
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                hint,
                Style::default().fg(p.muted),
            ))),
            list_area,
        );
    } else {
        // Reserve a scrollbar column when the list overflows its viewport.
        let viewport_rows = list_area.height as usize;
        let layout = pane_scroll_layout(list_area, list_area.width, visible.len(), viewport_rows);
        let content = layout.content_area;
        let viewport = content.height.max(1) as usize;
        let start = cursor.saturating_sub(viewport / 2);
        let end = (start + viewport).min(visible.len());
        let start = end.saturating_sub(viewport);

        let lines: Vec<Line> = visible[start..end]
            .iter()
            .enumerate()
            .map(|(row, &idx)| {
                let sql = &entries[idx];
                let selected = start + row == cursor;
                let prefix = if selected { "▸ " } else { "  " };
                let text = format!("{prefix}{}", history_one_line(sql));
                let style = if selected {
                    Style::default()
                        .fg(p.fg)
                        .bg(p.selection_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(p.fg)
                };
                Line::from(Span::styled(text, style))
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), content);

        if let Some(bar) = layout.v_scrollbar {
            let max_scroll = visible.len().saturating_sub(viewport);
            draw_vertical_pane_scrollbar(
                frame,
                bar,
                start,
                viewport,
                max_scroll,
                p,
                false,
            );
        }
    }

    // History footer hints from the shared builder (wrapped to the pane width).
    draw_footer(frame, theme, chunks[1], &hint);
}

impl HistoryState {
    /// Indices into `entries` matching the current search (all when no filter).
    fn visible_indices_for(&self, entries: &[String]) -> Vec<usize> {
        self.search.matching_indices(entries)
    }
}
