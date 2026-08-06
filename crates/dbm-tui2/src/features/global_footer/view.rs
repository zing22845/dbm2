//! Global footer feature rendering.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::common::utils::text_width;
use crate::common::view::hints::global_footer_text;
use crate::common::view::theme::Theme;

use super::state::FooterState;

/// Estimated number of terminal rows the footer occupies for `cols` columns.
///
/// The footer is one line of hints plus (when present) one wrapped status line,
/// so the hints line always reserves one row and the status may span several.
/// This is the same CJK-aware estimate the old TUI used to size its footer.
pub fn footer_height(state: &FooterState, cols: u16) -> u16 {
    let hints_rows = 1u16;
    let status_rows = if state.status.is_empty() {
        0
    } else {
        text_width::wrapped_line_count(&state.status, cols)
    };
    hints_rows + status_rows
}

/// Render the global footer bar: shortcut hints on the first line, followed by
/// the optional status line (muted). The hints come from
/// [`global_footer_text`] (the shared single source of truth); the status is
/// the only dynamically changing content.
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &FooterState) {
    // The hints line is static; the status line (if any) is muted via the
    // current palette's muted slot.
    let mut lines: Vec<Line<'_>> = vec![Line::from(global_footer_text(""))];
    if !state.status.is_empty() {
        lines.push(Line::from(Span::styled(
            state.status.clone(),
            Style::default().fg(theme.palette().muted),
        )));
    }
    frame.render_widget(ratatui::text::Text::from(lines), area);
}
