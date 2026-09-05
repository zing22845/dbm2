//! Performance monitor feature rendering.
//!
//! Sits on the right side of the global footer row: a single, compact line of
//! smoothed FPS and waste (redundant-redraw) percentage, right-aligned, in the
//! original dbm's `"x.x fps · x% waste"` format.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::common::view::theme::Theme;

use super::state::PerfState;

/// Text width of the compact perf readout (used to size the footer's right strip).
pub fn perf_width(_state: &PerfState) -> u16 {
    // " 120.0fps · 0% waste "
    23
}

/// Render the performance readout as one compact right-aligned line:
/// `x.x fps · x% waste`. `waste` is the share of redraws that changed nothing
/// on screen; it is color-coded by severity (green < 25%, yellow < 75%, else
/// red) instead of an opaque percentage.
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &PerfState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = theme.palette();
    let waste = state.redundancy_rate * 100.0;
    let waste_color = if waste >= 75.0 {
        Color::Red
    } else if waste >= 25.0 {
        Color::Yellow
    } else {
        Color::Green
    };
    let line = Line::from(vec![
        Span::styled(
            format!(" {:.1}fps", state.fps),
            Style::default().fg(p.muted),
        ),
        Span::raw(" · "),
        Span::styled(
            format!("{:.0}% waste", waste),
            Style::default().fg(waste_color),
        ),
        Span::raw(" "),
    ]);
    frame.render_widget(
        ratatui::widgets::Paragraph::new(line).alignment(Alignment::Right),
        area,
    );
}
