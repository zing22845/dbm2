//! Explorer objects (object tree) feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::ObjectsState;

/// Render the object tree with indentation and expansion markers.
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &ObjectsState) {
    let p = theme.palette();

    let mut lines = Vec::new();
    let inner_h = area.height.saturating_sub(2) as usize;
    for (vis, idx) in (state.scroll..state.rows.len()).enumerate() {
        if vis >= inner_h {
            break;
        }
        let row = &state.rows[idx];
        let focused = idx == state.cursor;
        let style = if focused {
            Style::default()
                .fg(p.selection)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg)
        };
        let marker = if row.expandable {
            if row.expanded { "▾" } else { "▸" }
        } else {
            "·"
        };
        let indent = "  ".repeat(row.depth);
        lines.push(Line::from(vec![
            Span::styled(format!("{indent}{marker} {}", row.label), style),
        ]));
    }
    if lines.is_empty() {
        let msg = if state.bound_connection.is_empty() {
            "(select a connection to browse objects)"
        } else {
            "(object tree pending — catalog fetch not yet wired)"
        };
        lines.push(Line::from(Span::styled(msg, Style::default().fg(p.muted))));
    }

    let block = Block::default()
        .title(" objects ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
