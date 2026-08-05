//! Engine selector feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::EngineState;

/// Render the engine selector: the selected engine (currently Postgres).
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &EngineState) {
    let p = theme.palette();
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
        .border_style(Style::default().fg(p.border_active));
    frame.render_widget(Paragraph::new(line).block(block), area);
}
