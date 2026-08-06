//! Engine selector feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::EngineState;

/// Render the engine selector: the selected engine (currently Postgres). The
/// border highlights only when the engine pane owns focus.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &EngineState,
    focus: crate::app_shell::pane::DiscoverPane,
) {
    use crate::common::view::hints::{discover_engine_footer_text, draw_pane_footer};
    let p = theme.palette();
    let focused = focus == crate::app_shell::pane::DiscoverPane::Engine;
    let line = Line::from(vec![
        Span::styled(" engine: ", Style::default().fg(p.muted)),
        Span::styled(
            state.engine.to_string(),
            Style::default()
                .fg(p.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" (e to focus)", Style::default().fg(p.muted)),
    ]);
    let block = Block::default()
        .title(" discover ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { p.border_active } else { p.border }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let footer = discover_engine_footer_text();
    let footer_h = if footer.is_empty() { 0 } else { 1 };
    let body_h = inner.height.saturating_sub(footer_h);
    if body_h > 0 {
        let body = Rect::new(inner.x, inner.y, inner.width, body_h);
        frame.render_widget(Paragraph::new(line), body);
    }
    if footer_h > 0 {
        let footer_area = Rect::new(inner.x, inner.y + body_h, inner.width, footer_h);
        draw_pane_footer(frame, theme, footer_area, &footer);
    }
}
