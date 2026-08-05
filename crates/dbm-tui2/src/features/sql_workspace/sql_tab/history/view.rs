//! History feature rendering: the history list pane with `/` search title and
//! the detail preview.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::components::search::pane_search_title_line;
use crate::common::view::theme::Theme;

use super::state::HistoryState;
use super::store::history_one_line;

/// Render the history list pane for a tab's connection.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &HistoryState,
    instance: &str,
    connection: &str,
) {
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
        Some(area.width.saturating_sub(4)),
        None,
        Some(Style::default().fg(p.accent)),
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border_active))
        .style(Style::default().bg(p.surface));
    let inner = block.inner(area);
    frame.render_widget(block, area);

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
            inner,
        );
        return;
    }

    let viewport = inner.height.max(1) as usize;
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
                    .fg(p.selection)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(p.fg)
            };
            Line::from(Span::styled(text, style))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

impl HistoryState {
    /// Indices into `entries` matching the current search (all when no filter).
    fn visible_indices_for(&self, entries: &[String]) -> Vec<usize> {
        self.search.matching_indices(entries)
    }
}
