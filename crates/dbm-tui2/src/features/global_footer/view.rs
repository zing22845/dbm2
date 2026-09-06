//! Global footer feature rendering.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::{Paragraph, Wrap};

use crate::common::utils::text_width;
use crate::common::view::hints::global_footer_text;
use crate::common::view::theme::Theme;

use super::state::FooterState;

/// Render the global footer bar: shortcut hints, followed by the optional
/// status line (muted). The hints come from [`global_footer_text`] (the shared
/// single source of truth); the status is the only dynamically changing
/// content. Both wrap to the terminal width.
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &FooterState) {
    let p = theme.palette();
    let hints_h = text_width::wrapped_line_count(&global_footer_text(""), area.width);
    let (hints_area, status_area) = if state.status.is_empty() {
        (area, Rect::default())
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(hints_h), Constraint::Min(0)])
            .split(area);
        (chunks[0], chunks[1])
    };

    // The hints line wraps to the terminal width instead of clipping. Its color
    // matches every other footer (the muted slot), not the default foreground.
    frame.render_widget(
        Paragraph::new(global_footer_text(""))
            .style(Style::default().fg(p.muted))
            .wrap(Wrap { trim: false }),
        hints_area,
    );
    // The status line (if any) is muted via the current palette's muted slot.
    if !state.status.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                state.status.clone(),
                Style::default().fg(p.muted),
            ))
            .wrap(Wrap { trim: false }),
            status_area,
        );
    }
}
