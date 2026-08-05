//! Instance overview feature rendering.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::OverviewState;

/// Render the instance overview panel: name/host/port and status.
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &OverviewState) {
    let p = theme.palette();
    let mut lines = Vec::new();
    match &state.instance {
        Some(inst) => {
            lines.push(Line::from(vec![
                Span::styled("Name:   ", Style::default().fg(p.muted)),
                Span::styled(&inst.name, Style::default().fg(p.fg)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Host:   ", Style::default().fg(p.muted)),
                Span::styled(format!("{}:{}", inst.host, inst.port), Style::default().fg(p.fg)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Engine: ", Style::default().fg(p.muted)),
                Span::styled(inst.engine.to_string(), Style::default().fg(p.accent)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(p.muted)),
                Span::styled("managed", Style::default().fg(p.success)),
            ]));
        }
        None => {
            lines.push(Line::from(Span::styled(
                "(select an instance in the explorer)",
                Style::default().fg(p.muted),
            )));
        }
    }
    let block = Block::default()
        .title(" overview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
