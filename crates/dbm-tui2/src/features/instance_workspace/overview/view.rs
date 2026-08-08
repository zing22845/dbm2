//! Instance overview feature rendering.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::hints::{draw_pane_footer, instance_workspace_footer_text};
use crate::common::view::theme::Theme;
use crate::app_shell::nav::IwPane;

use super::state::OverviewState;

/// Render the instance overview panel: name/host/port and status. `focused`
/// highlights the pane border when the instance workspace owns the shell focus
/// (matching the connections sub-pane). A pane footer hint line occupies the
/// bottom row inside the border.
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &OverviewState, focused: bool) {
    let p = theme.palette();

    // The block is drawn over `area`; its inner area is split into a body and a
    // footer hint line at the bottom, both *inside* the pane's border.
    let block = Block::default()
        .title(" overview ")
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    frame.render_widget(&block, area);
    let inner = block.inner(area);
    let (body, footer_area) = if inner.height > 1 {
        let h = inner.height.saturating_sub(1);
        (
            Rect {
                x: inner.x,
                y: inner.y,
                width: inner.width,
                height: h,
            },
            Rect {
                x: inner.x,
                y: inner.y.saturating_add(h),
                width: inner.width,
                height: 1,
            },
        )
    } else {
        (inner, Rect::default())
    };

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
    frame.render_widget(Paragraph::new(lines), body);
    // Pane footer (inside the border): Refresh/Unregister/H-Scroll.
    draw_pane_footer(
        frame,
        theme,
        footer_area,
        &instance_workspace_footer_text(IwPane::Overview),
    );
}
