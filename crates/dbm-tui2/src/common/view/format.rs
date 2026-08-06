//! Results-grid layout math, cell display, and scroll helpers.
//!
//! Pure presentation logic for the query-results table: auto-sizing column
//! widths, truncating cell text to fit, mapping pointer positions to cells, and
//! computing scroll limits. It is deliberately feature-free: it only needs a
//! minimal `ResultColumn` projection (name + type labels), so any feature's
//! column type can drive it without `common` depending on a feature.

use crate::common::utils::text_width;

/// Visible field separator for TUI text (tabs do not render reliably in ratatui).
pub const FIELD_SEP: &str = "  ";

pub const DEFAULT_RESULTS_COL_WIDTH: u16 = 14;
pub const MIN_RESULTS_COL_WIDTH: u16 = 4;
pub const MAX_RESULTS_COL_WIDTH: u16 = 80;
/// Auto-sized columns never exceed this width; longer values truncate with ellipsis.
pub const AUTO_RESULTS_COL_WIDTH_CAP: u16 = 36;
/// One display column is reserved for the vertical border between headers/cells.
pub const RESULTS_COL_BORDER_WIDTH: u16 = 1;
/// `truncate_cell_display` needs one extra column to avoid truncating content that fits.
pub const RESULTS_COL_ELLIPSIS_RESERVE: u16 = 1;
pub const RESULTS_COL_SPACING: u16 = 0;
pub const RESULTS_COL_WIDTH_STEP: i16 = 2;

/// Minimal projection of a result column used for layout.
///
/// Implement this for a feature's column type (e.g. `ColumnInfo`) to feed the
/// layout functions here without `common` depending on any feature.
pub trait ResultColumn {
    fn name(&self) -> &str;
    fn type_name(&self) -> &str;
    fn type_display(&self) -> &str;
}

/// Compute the auto-sized display width for every column of a result.
pub fn init_results_layout<C: ResultColumn>(columns: &[C], rows: &[Vec<String>]) -> Vec<u16> {
    columns
        .iter()
        .enumerate()
        .map(|(col_idx, col)| column_width_for_result(col, rows, col_idx))
        .collect()
}

fn column_width_for_result<C: ResultColumn>(col: &C, rows: &[Vec<String>], col_idx: usize) -> u16 {
    let measured = column_measured_width(col, rows, col_idx).min(AUTO_RESULTS_COL_WIDTH_CAP);
    results_col_width_for_measured(measured)
}

/// Widest display width among column name, type label, and cell values.
fn column_measured_width<C: ResultColumn>(col: &C, rows: &[Vec<String>], col_idx: usize) -> u16 {
    let mut max_w = str_display_width(col.name());
    max_w = max_w.max(str_display_width(&column_type_label(col)));
    for row in rows {
        if let Some(value) = row.get(col_idx) {
            max_w = max_w.max(str_display_width(value));
        }
    }
    max_w.min(usize::from(u16::MAX)) as u16
}

fn results_col_width_for_measured(measured: u16) -> u16 {
    clamp_col_width(
        measured
            .saturating_add(RESULTS_COL_BORDER_WIDTH)
            .saturating_add(RESULTS_COL_ELLIPSIS_RESERVE),
    )
}

fn clamp_col_width(width: u16) -> u16 {
    width.clamp(MIN_RESULTS_COL_WIDTH, MAX_RESULTS_COL_WIDTH)
}

pub fn friendly_type_name(type_name: &str) -> String {
    type_name.to_ascii_lowercase()
}

pub fn column_type_label<C: ResultColumn>(meta: &C) -> String {
    let display = meta.type_display();
    if display.is_empty() {
        friendly_type_name(meta.type_name())
    } else {
        friendly_type_name(display)
    }
}

/// Truncate cell text to fit column display width (unicode-aware).
pub fn truncate_cell_display(text: &str, width: u16) -> String {
    text_width::truncate(text, width as usize)
}

/// Skip leading display columns, then truncate (unicode-aware).
pub fn truncate_cell_display_from(text: &str, skip: u16, width: u16) -> String {
    text_width::truncate_from(text, skip as usize, width as usize)
}

fn str_display_width(text: &str) -> usize {
    text_width::width(text)
}

pub fn cell_display_width(text: &str) -> usize {
    text_width::width(text)
}

pub fn max_cell_text_skip(text: &str, visible_width: u16) -> u16 {
    if visible_width == 0 {
        return 0;
    }
    let total = str_display_width(text);
    let vis = visible_width as usize;
    if total <= vis {
        return 0;
    }
    total.saturating_sub(vis).min(u16::MAX as usize) as u16
}

/// Visible text width and table-scroll skip for a column in the results grid.
pub fn results_col_text_view(
    col: usize,
    col_widths: &[u16],
    viewport_width: u16,
    h_scroll: u16,
) -> Option<ResultsColTextView> {
    if viewport_width == 0 {
        return None;
    }
    let raw_w = col_widths
        .get(col)
        .copied()
        .unwrap_or(DEFAULT_RESULTS_COL_WIDTH) as usize;
    let col_left = col_x_start(col, col_widths);
    let col_right = col_left + raw_w;
    let view_left = h_scroll as usize;
    let view_right = view_left + viewport_width as usize;
    if col_right <= view_left || col_left >= view_right {
        return None;
    }
    let vis_left = col_left.max(view_left);
    let vis_right = col_right.min(view_right);
    let text_start = col_left;
    let text_end_exclusive = col_right.saturating_sub(1);
    let text_vis_left = vis_left.max(text_start);
    let text_vis_right = vis_right.min(text_end_exclusive);
    let text_w = if text_vis_right > text_vis_left {
        (text_vis_right - text_vis_left) as u16
    } else {
        0
    };
    Some(ResultsColTextView {
        text_w,
        table_text_skip: text_vis_left.saturating_sub(text_start) as u16,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultsColTextView {
    pub text_w: u16,
    pub table_text_skip: u16,
}

pub fn display_width_char_prefix(text: &str, char_offset: usize) -> usize {
    text_width::prefix_width(text, char_offset)
}

pub const RESULTS_HEADER_CONTENT_HEIGHT: u16 = 2;
pub const RESULTS_HEADER_BORDER_HEIGHT: u16 = 1;
pub const RESULTS_HEADER_HEIGHT: u16 = RESULTS_HEADER_CONTENT_HEIGHT + RESULTS_HEADER_BORDER_HEIGHT;
pub const RESULTS_ROW_CONTENT_HEIGHT: u16 = 1;
pub const RESULTS_ROW_BORDER_HEIGHT: u16 = 1;
pub const RESULTS_ROW_HEIGHT: u16 = RESULTS_ROW_CONTENT_HEIGHT + RESULTS_ROW_BORDER_HEIGHT;
/// Pinned left gutter for Results dirty markers (`+`/`-`/`~`) while editing.
pub const RESULTS_DIRTY_GUTTER_WIDTH: u16 = 1;

pub fn results_table_width(col_widths: &[u16]) -> u16 {
    if col_widths.is_empty() {
        return 0;
    }
    col_x_end(col_widths.len() - 1, col_widths) as u16
}

pub fn results_cell_popup_line_count(body: &str, width: u16) -> usize {
    if body.is_empty() {
        return 1;
    }
    let w = width.max(1) as usize;
    body.lines()
        .map(|line| wrapped_text_line_count(line, w))
        .sum::<usize>()
        .max(1)
}

fn wrapped_text_line_count(line: &str, width: usize) -> usize {
    if line.is_empty() {
        return 1;
    }
    let mut lines = 1usize;
    let mut used = 0usize;
    for ch in line.chars() {
        let cw = char_display_width(ch);
        if used + cw > width {
            lines += 1;
            used = cw;
        } else {
            used += cw;
        }
    }
    lines
}

pub fn results_max_h_scroll(table_width: u16, viewport_width: u16) -> u16 {
    table_width.saturating_sub(viewport_width)
}

pub fn results_visible_data_rows(viewport_height: u16) -> usize {
    usize::from(viewport_height.saturating_sub(RESULTS_HEADER_HEIGHT) / RESULTS_ROW_HEIGHT)
}

pub fn results_max_v_scroll(row_count: usize, viewport_height: u16) -> usize {
    let visible = results_visible_data_rows(viewport_height).max(1);
    row_count.saturating_sub(visible)
}

pub fn clamp_results_v_scroll_offset(
    offset: usize,
    row_count: usize,
    viewport_height: u16,
) -> usize {
    offset.min(results_max_v_scroll(row_count, viewport_height))
}

#[allow(clippy::too_many_arguments)]
pub fn results_cell_at_point(
    rel_x: usize,
    rel_y: u16,
    row_count: usize,
    col_count: usize,
    row_offset: usize,
    h_scroll: u16,
    col_widths: &[u16],
    gutter: u16,
) -> Option<(usize, usize)> {
    if col_count == 0 || row_count == 0 {
        return None;
    }
    if rel_y < RESULTS_HEADER_HEIGHT {
        return None;
    }
    let rel_row = rel_y.saturating_sub(RESULTS_HEADER_HEIGHT);
    let row = row_offset + usize::from(rel_row / RESULTS_ROW_HEIGHT);
    if row >= row_count {
        return None;
    }
    let data_x = rel_x.saturating_sub(gutter as usize);
    let scrolled_x = data_x.saturating_add(h_scroll as usize);
    let col = column_at_x(scrolled_x, col_widths).min(col_count - 1);
    Some((row, col))
}

pub fn col_x_start(col_idx: usize, widths: &[u16]) -> usize {
    widths
        .iter()
        .take(col_idx)
        .map(|w| *w as usize + RESULTS_COL_SPACING as usize)
        .sum()
}

pub fn col_x_end(col_idx: usize, widths: &[u16]) -> usize {
    col_x_start(col_idx, widths)
        + widths
            .get(col_idx)
            .copied()
            .unwrap_or(DEFAULT_RESULTS_COL_WIDTH) as usize
}

pub fn column_at_x(rel_col: usize, widths: &[u16]) -> usize {
    if widths.is_empty() {
        return 0;
    }
    for (idx, _) in widths.iter().enumerate() {
        if rel_col < col_x_end(idx, widths) {
            return idx;
        }
    }
    widths.len() - 1
}

/// Column index whose width should be resized when dragging near a header border.
pub fn resize_hit_column(rel_x: usize, rel_y: u16, h_scroll: u16, widths: &[u16]) -> Option<usize> {
    if rel_y >= RESULTS_HEADER_CONTENT_HEIGHT || widths.is_empty() {
        return None;
    }
    let scrolled_x = rel_x.saturating_add(h_scroll as usize);
    for col in 0..widths.len() {
        let right = col_x_end(col, widths).saturating_sub(1);
        if scrolled_x.abs_diff(right) <= 1 {
            return Some(col);
        }
    }
    for col in 1..widths.len() {
        let left = col_x_start(col, widths);
        if scrolled_x.abs_diff(left) <= 1 {
            return Some(col - 1);
        }
    }
    None
}

pub(crate) fn char_display_width(ch: char) -> usize {
    text_width::char_width(ch)
}

/// Plain text of a result: tab-separated header + rows, or an affected-row
/// message when the query returned no columns.
pub fn results_plain_text(
    columns: &[impl AsRef<str>],
    rows: &[Vec<String>],
    rows_affected: Option<u64>,
) -> String {
    if columns.is_empty() {
        return rows_affected
            .map(|n| format!("{n} row(s) affected"))
            .unwrap_or_else(|| "Done".to_string());
    }

    let mut lines = Vec::new();
    lines.push(
        columns
            .iter()
            .map(|c| c.as_ref())
            .collect::<Vec<_>>()
            .join("\t"),
    );
    for row in rows {
        lines.push(row.join("\t"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal `ResultColumn` used by the tests below.
    struct Col {
        name: String,
        type_name: String,
        type_display: String,
    }
    impl Col {
        fn new(name: &str, type_name: &str, type_display: &str) -> Self {
            Col {
                name: name.into(),
                type_name: type_name.into(),
                type_display: type_display.into(),
            }
        }
    }
    impl ResultColumn for Col {
        fn name(&self) -> &str {
            &self.name
        }
        fn type_name(&self) -> &str {
            &self.type_name
        }
        fn type_display(&self) -> &str {
            &self.type_display
        }
    }

    #[test]
    fn truncate_cell_display_ellipsis() {
        let text = "科学研究管理系统";
        let rendered = truncate_cell_display(text, 8);
        assert!(rendered.ends_with('…'));
    }

    #[test]
    fn truncate_cell_display_from_skips_then_truncates() {
        let text = "abcdefgh";
        let rendered = truncate_cell_display_from(text, 3, 4);
        assert_eq!(rendered, "def…");
    }

    #[test]
    fn truncate_cell_display_from_shows_tail_without_ellipsis_at_end() {
        let text = "abcdefghijklmnopqrst";
        let rendered = truncate_cell_display_from(text, 15, 5);
        assert_eq!(rendered, "pqrst");
    }

    #[test]
    fn results_max_v_scroll_computes_offset() {
        // height 23 → header 3 + 10 data rows × 2 lines = fits 10 rows
        assert_eq!(super::results_max_v_scroll(10, 23), 0);
        assert_eq!(super::results_max_v_scroll(20, 23), 10);
    }

    #[test]
    fn init_results_layout_per_column() {
        let result_columns = [
            Col::new("id", "int8", ""),
            Col::new("note", "text", ""),
            Col::new("active", "bool", ""),
        ];
        let rows = vec![
            vec!["42".into(), "hello".into(), "true".into()],
            vec!["9999999999999999999".into(), "x".into(), "false".into()],
        ];
        assert_eq!(
            init_results_layout(&result_columns, &rows),
            vec![
                results_col_width_for_measured(19),
                results_col_width_for_measured(5),
                results_col_width_for_measured(6),
            ]
        );
    }

    #[test]
    fn init_results_layout_expands_to_content_and_caps_at_36() {
        let long = "a".repeat(50);
        let columns = [Col::new("payload", "text", "")];
        let rows = vec![vec![long]];
        assert_eq!(
            init_results_layout(&columns, &rows),
            vec![results_col_width_for_measured(AUTO_RESULTS_COL_WIDTH_CAP)]
        );
    }

    #[test]
    fn init_results_layout_fits_name_type_and_cells_without_truncation() {
        let result_columns = [
            Col::new("id", "int4", ""),
            Col::new("状态", "int2", ""),
            Col::new("创建时间", "timestamp", ""),
        ];
        let rows = vec![vec![
            "107".into(),
            "1569".into(),
            "2028-07-10 19:35:00".into(),
        ]];
        let widths = init_results_layout(&result_columns, &rows);
        let cases = [
            ("int4", &result_columns[0]),
            ("int2", &result_columns[1]),
            ("timestamp", &result_columns[2]),
        ];
        for (col_idx, (type_label, col)) in cases.iter().enumerate() {
            let text_w = widths[col_idx] - RESULTS_COL_BORDER_WIDTH;
            assert_eq!(
                truncate_cell_display(type_label, text_w),
                *type_label,
                "type label for column {}",
                col.name
            );
            assert_eq!(
                truncate_cell_display(&rows[0][col_idx], text_w),
                rows[0][col_idx],
                "cell value for column {}",
                col.name
            );
        }
    }

    #[test]
    fn results_col_width_for_measured_reserves_ellipsis_slot() {
        let width = results_col_width_for_measured(4);
        let text_w = width - RESULTS_COL_BORDER_WIDTH;
        assert_eq!(truncate_cell_display("2041", text_w), "2041");
    }

    #[test]
    fn init_results_layout_timestamp_matches_content() {
        let ts = "2023-10-13 19:00:59";
        let columns = [Col::new("创建时间", "timestamp", "")];
        let rows = vec![vec![ts.into()]];
        assert_eq!(
            init_results_layout(&columns, &rows),
            vec![results_col_width_for_measured(str_display_width(ts) as u16)]
        );
    }

    #[test]
    fn max_cell_text_skip_zero_when_content_fits() {
        assert_eq!(max_cell_text_skip("short", 10), 0);
    }

    #[test]
    fn max_cell_text_skip_positive_when_truncated() {
        assert!(max_cell_text_skip("hello world", 5) > 0);
    }

    #[test]
    fn results_cell_at_point_skips_gutter() {
        let widths = [10u16, 10];
        let cell = results_cell_at_point(
            0,
            RESULTS_HEADER_HEIGHT,
            2,
            2,
            0,
            0,
            &widths,
            1,
        );
        assert_eq!(cell, Some((0, 0)));
        let cell = results_cell_at_point(
            1 + 5,
            RESULTS_HEADER_HEIGHT,
            2,
            2,
            0,
            0,
            &widths,
            1,
        );
        assert_eq!(cell, Some((0, 0)));
    }

    #[test]
    fn results_plain_text_affected_rows_when_no_columns() {
        let rows: Vec<Vec<String>> = Vec::new();
        assert_eq!(results_plain_text(&[] as &[String], &rows, Some(3)), "3 row(s) affected");
        assert_eq!(results_plain_text(&[] as &[String], &rows, None), "Done");
    }
}
