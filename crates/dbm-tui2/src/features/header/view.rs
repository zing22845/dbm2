//! Header feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Style, Modifier};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::HeaderState;

/// Render the header: a bordered app title bar with an action-button row.
///
/// The header currently has a single `Discover` button; when it is the focused
/// button it is highlighted with the accent/selection slot.
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &HeaderState) {
    let p = theme.palette();

    // The `Discover` button is focused when the header cursor points at it.
    let discover_focused = state.button == 0;
    let discover_style = if discover_focused {
        Style::default()
            .fg(p.selection)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(p.fg)
    };

    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled(" Discover ", discover_style),
        Span::raw("  "),
        Span::styled("ENTER: activate", Style::default().fg(p.muted)),
    ]);

    let block = Block::default()
        .title(" dbm ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border_active));
    frame.render_widget(Paragraph::new(line).block(block), area);
}
