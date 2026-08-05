//! Discovery results feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::ResultsState;

/// Render the results list of discovered instances.
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &ResultsState) {
    let p = theme.palette();

    let mut lines = Vec::new();
    if state.is_empty() {
        lines.push(Line::from(Span::styled(
            "(scan to discover instances)",
            Style::default().fg(p.muted),
        )));
    } else {
        let inner_h = area.height.saturating_sub(2) as usize;
        for (vis, idx) in (state.scroll..state.items.len()).enumerate() {
            if vis >= inner_h {
                break;
            }
            let item = &state.items[idx];
            let row_focused = idx == state.cursor;
            let checked = if state.selected.contains(&idx) { "✓" } else { " " };
            let style = if row_focused {
                Style::default()
                    .fg(p.selection)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(p.fg)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("[{checked}]"), Style::default().fg(p.accent)),
                Span::styled(format!("  {}:{}", item.host, item.port), style),
                Span::styled(
                    format!("  ({})", item.confidence.as_str()),
                    Style::default().fg(p.muted),
                ),
            ]));
        }
    }

    let block = Block::default()
        .title(" results ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
