//! Performance monitor feature rendering.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::PerfState;

/// Render the performance readout: smoothed FPS and redundant-redraw ratio.
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &PerfState) {
    let p = theme.palette();
    let fps_line = Line::from(vec![
        Span::styled("FPS", Style::default().fg(p.accent)),
        Span::styled(format!(" {:.0}", state.fps), Style::default().fg(p.fg)),
    ]);
    let red_line = Line::from(vec![
        Span::styled("Redundancy", Style::default().fg(p.accent)),
        Span::styled(
            format!(" {:.0}%", state.redundancy_rate * 100.0),
            Style::default().fg(p.fg),
        ),
    ]);
    frame.render_widget(
        ratatui::widgets::Paragraph::new(vec![fps_line, red_line]),
        area,
    );
}
