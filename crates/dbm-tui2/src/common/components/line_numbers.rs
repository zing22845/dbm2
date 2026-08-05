//! Absolute line-number gutter for multi-line panes (History Detail, Results
//! Detail). Width formula matches `edtui::LineNumbers::Absolute`.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// Gutter width for `total_logical_lines` (digits + trailing pad), same as edtui.
pub fn gutter_width(total_logical_lines: usize) -> u16 {
    let digits = total_logical_lines.max(1).to_string().len();
    (digits + 1) as u16
}

pub fn gutter_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// Right-aligned `line_no` plus a trailing pad space (edtui Absolute gutter).
pub fn format_gutter(line_no: usize, gutter_w: u16) -> String {
    let digits_w = gutter_w.saturating_sub(1).max(1) as usize;
    format!("{line_no:>digits_w$} ")
}

pub fn empty_gutter(gutter_w: u16) -> String {
    " ".repeat(gutter_w as usize)
}

/// Text width available after reserving the line-number gutter.
pub fn text_width_after_gutter(area_width: u16, total_logical_lines: usize) -> u16 {
    area_width
        .saturating_sub(gutter_width(total_logical_lines))
        .max(1)
}

/// Prefix wrapped display rows for one logical line with an absolute number on
/// the first wrap row only.
pub fn prefix_wrapped_line(
    line_no: usize,
    gutter_w: u16,
    wrapped_rows: Vec<Line<'static>>,
) -> Vec<Line<'static>> {
    let style = gutter_style();
    wrapped_rows
        .into_iter()
        .enumerate()
        .map(|(j, row)| {
            let prefix = if j == 0 {
                Span::styled(format_gutter(line_no, gutter_w), style)
            } else {
                Span::raw(empty_gutter(gutter_w))
            };
            let mut spans = vec![prefix];
            spans.extend(row.spans);
            Line::from(spans)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gutter_width_matches_edtui_digits_plus_one() {
        assert_eq!(gutter_width(1), 2);
        assert_eq!(gutter_width(9), 2);
        assert_eq!(gutter_width(10), 3);
        assert_eq!(gutter_width(99), 3);
        assert_eq!(gutter_width(100), 4);
    }

    #[test]
    fn format_gutter_right_aligns_with_trailing_space() {
        assert_eq!(format_gutter(1, 2), "1 ");
        assert_eq!(format_gutter(1, 3), " 1 ");
        assert_eq!(format_gutter(12, 3), "12 ");
    }
}
