//! Explorer instances (connection tree) feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::common::view::theme::Theme;

use super::state::InstancesState;

/// Render the instances connection tree. `region_focused` controls the border
/// color so the shell focus is visible (active border vs. muted border).
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &InstancesState,
    region_focused: bool,
) {
    let p = theme.palette();

    let mut lines = Vec::new();
    let inner_h = area.height.saturating_sub(2) as usize;
    let mut row = 0usize;
    'outer: for node in &state.nodes {
        let instance_name = node
            .instance
            .as_ref()
            .map(|i| i.name.clone())
            .unwrap_or_default();
        let focused = row == state.cursor;
        let style = if focused {
            Style::default()
                .fg(p.selection)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg)
        };
        let marker = if node.expanded { "▾" } else { "▸" };
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} {instance_name}"), style),
        ]));
        row += 1;
        if row > inner_h {
            break;
        }
        if node.expanded {
            for conn in &node.connections {
                let conn_focused = row == state.cursor;
                let cstyle = if conn_focused {
                    Style::default()
                        .fg(p.accent)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(p.fg_dim)
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("    └ {}/{}", conn.name, conn.database), cstyle),
                ]));
                row += 1;
                if row > inner_h {
                    break 'outer;
                }
            }
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no instances — run discover)",
            Style::default().fg(p.muted),
        )));
    }

    let border_color = if region_focused { p.border_active } else { p.border };
    let block = Block::default()
        .title(" instances ")
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
