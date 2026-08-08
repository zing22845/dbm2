//! Explorer objects (object tree) feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::common::view::theme::Theme;

use super::state::ObjectsState;

/// Render the object tree with indentation and expansion markers.
/// `region_focused` controls the border color so the shell focus is visible.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ObjectsState,
    region_focused: bool,
) {
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
            "(no objects — press Enter on a connection to load the catalog)"
        };
        lines.push(Line::from(Span::styled(msg, Style::default().fg(p.muted))));
    }

    let border_color = if region_focused { p.border_active } else { p.border };
    let block = Block::default()
        .title(" objects ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));
    // Clamp the horizontal scroll to the widest content row so that a narrow
    // tree (fully visible) cannot be panned into blank space. `h_scroll` only
    // takes effect when the longest rendered line exceeds the text viewport.
    let viewport_w = area.width.saturating_sub(2) as usize;
    let max_row_w = lines
        .iter()
        .map(|l| UnicodeWidthStr::width(l.to_string().as_str()))
        .max()
        .unwrap_or(0);
    let effective_h = state
        .h_scroll
        .min(max_row_w.saturating_sub(viewport_w) as u16);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((0, effective_h));
    frame.render_widget(paragraph, area);
}
