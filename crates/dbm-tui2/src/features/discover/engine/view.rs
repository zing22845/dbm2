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
    focus: crate::app_shell::nav::DiscoverPane,
) {
    use crate::common::utils::text_width::wrapped_line_count;
    use crate::common::view::hints::{discover_engine_footer_text, draw_pane_footer};
    let p = theme.palette();
    let focused = focus == crate::app_shell::nav::DiscoverPane::Engine;
    let line = Line::from(vec![
        Span::styled(" engine: ", Style::default().fg(p.muted)),
        Span::styled(
            state.engine.to_string(),
            Style::default()
                .fg(p.accent)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let block = Block::default()
        .title(" Engine ")
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let footer = discover_engine_footer_text(state.status.as_deref());
    // The status note wraps under a narrow pane, so size the footer to its
    // actual wrapped line count, never crowding out the whole body.
    let footer_h = if footer.is_empty() {
        0
    } else {
        wrapped_line_count(&footer, inner.width)
            .max(1)
            .min(inner.height.saturating_sub(1).max(1))
    };
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
