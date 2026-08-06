//! Performance monitor feature rendering.
//!
//! Sits on the right side of the global footer row: a single, compact line of
//! smoothed FPS and redundant-redraw ratio, right-aligned.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::PerfState;

/// Text width of the compact perf readout (used to size the footer's right strip).
pub fn perf_width(_state: &PerfState) -> u16 {
    // "FPS 999  Redundancy 100%"
    24
}

/// Render the performance readout as one compact right-aligned line.
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &PerfState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = theme.palette();
    let line = Line::from(vec![
        Span::styled("FPS", Style::default().fg(p.accent)),
        Span::styled(format!(" {:.0}", state.fps), Style::default().fg(p.fg)),
        Span::styled("  Red", Style::default().fg(p.accent)),
        Span::styled(
            format!(" {:.0}%", state.redundancy_rate * 100.0),
            Style::default().fg(p.fg),
        ),
    ]);
    frame.render_widget(
        ratatui::widgets::Paragraph::new(line).alignment(Alignment::Right),
        area,
    );
}
