//! Cell-level search matching and highlight rendering for the results grid.
//!
//! Unlike the history/objects panes whose search filters whole rows, results
//! search operates on individual cells: a query matches substrings inside any
//! cell, producing a flat ordered list of `ResultsSearchMatch { row, col,
//! start }`. The current match drives the cell cursor and a count / offset
//! read-out, and each visible cell highlights its matching substring.
//!
//! Mirrors the original dbm's `results/search.rs`.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::common::components::search::{PaneSearch, TextSearchOptions, find_match_starts};
use crate::common::utils::text_width;
use crate::common::view::format::{display_width_char_prefix, truncate_cell_display_from};

use super::super::state::QueryResultData;

/// A single occurrence of the query inside a results cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultsSearchMatch {
    pub row: usize,
    pub col: usize,
    /// Character index in the cell value where the match starts.
    pub start: usize,
}

/// Every non-overlapping match of `query` across the result, limited to
/// `column` when `Some` (scope) or all columns when `None`. Matches are
/// ordered row-by-row then column-by-column.
pub fn find_matches(
    result: &QueryResultData,
    query: &str,
    column: Option<usize>,
    opts: TextSearchOptions,
) -> Vec<ResultsSearchMatch> {
    let query = query.trim();
    if query.is_empty() || result.rows.is_empty() || result.columns.is_empty() {
        return Vec::new();
    }
    let cols: Vec<usize> = match column {
        Some(c) if c < result.columns.len() => vec![c],
        Some(_) => return Vec::new(),
        None => (0..result.columns.len()).collect(),
    };

    let mut matches = Vec::new();
    for row in 0..result.rows.len() {
        for &col in &cols {
            let value = result
                .rows
                .get(row)
                .and_then(|r| r.get(col))
                .map(String::as_str)
                .unwrap_or("");
            for start in find_match_starts(value, query, opts) {
                matches.push(ResultsSearchMatch { row, col, start });
            }
        }
    }
    matches
}

/// The match start char-indices within one cell, for highlight rendering.
pub fn match_starts_in_cell(matches: &[ResultsSearchMatch], row: usize, col: usize) -> Vec<usize> {
    matches
        .iter()
        .filter(|m| m.row == row && m.col == col)
        .map(|m| m.start)
        .collect()
}

/// Render one cell as a line that highlights every query match, colouring the
/// current match (when on this cell) distinctly. `skip` is the table-level
/// text skip for the column and `width` the visible cell width. `base_style`
/// styles the non-matching text, `other_match_style` the non-current matches,
/// and `current_match_style` the current match — the caller supplies the
/// theme-derived styles so this pure helper stays theme-agnostic. Falls back
/// to plain truncation when there is nothing to highlight.
#[allow(clippy::too_many_arguments)]
pub fn cell_highlight_line(
    text: &str,
    skip: u16,
    width: u16,
    query: &str,
    match_starts: &[usize],
    current_start: Option<usize>,
    base_style: Style,
    other_match_style: Style,
    current_match_style: Style,
) -> Line<'static> {
    let query = query.trim();
    if width == 0 || query.is_empty() || match_starts.is_empty() {
        return Line::from(Span::styled(
            truncate_cell_display_from(text, skip, width),
            base_style,
        ));
    }

    let query_len = query.chars().count();
    let visible = visible_char_range(text, skip, width);
    let Some((vis_start, vis_end)) = visible else {
        return Line::from(Span::styled(
            truncate_cell_display_from(text, skip, width),
            base_style,
        ));
    };

    let mut spans = Vec::new();
    let mut pos = vis_start;
    while pos < vis_end {
        let match_at = match_starts.iter().copied().find(|&start| start >= pos);
        let Some(start) = match_at else {
            push_char_range(text, pos, vis_end, base_style, &mut spans);
            break;
        };
        if start > pos {
            push_char_range(text, pos, start, base_style, &mut spans);
        }
        let end = (start + query_len).min(vis_end);
        let style = if current_start == Some(start) {
            current_match_style
        } else {
            other_match_style
        };
        push_char_range(text, start, end, style, &mut spans);
        pos = end;
    }

    if spans.is_empty() {
        Line::from(Span::styled(
            truncate_cell_display_from(text, skip, width),
            base_style,
        ))
    } else {
        Line::from(spans)
    }
}

/// Char range `[start, end)` of `text` that falls within the visible window
/// of `width` cells starting at display `skip`.
fn visible_char_range(text: &str, skip: u16, width: u16) -> Option<(usize, usize)> {
    if width == 0 {
        return None;
    }
    let vis_start = char_index_at_display_skip(text, skip);
    let vis_end = char_index_at_display_skip(text, skip.saturating_add(width));
    Some((vis_start, vis_end.max(vis_start)))
}

/// Char index into `text` corresponding to the start of the display window at
/// `skip` cells (clamped to the text length).
fn char_index_at_display_skip(text: &str, skip: u16) -> usize {
    let target = skip as usize;
    let mut skipped = 0usize;
    let mut char_idx = 0usize;
    for ch in text.chars() {
        if skipped >= target {
            break;
        }
        let w = text_width::char_width(ch);
        if skipped + w > target {
            break;
        }
        skipped += w;
        char_idx += 1;
    }
    char_idx.min(text.chars().count())
}

/// Append chars `[start, end)` of `text` as one styled span.
fn push_char_range(
    text: &str,
    start: usize,
    end: usize,
    style: Style,
    spans: &mut Vec<Span<'static>>,
) {
    if start >= end {
        return;
    }
    let slice: String = text.chars().skip(start).take(end - start).collect();
    if !slice.is_empty() {
        spans.push(Span::styled(slice, style));
    }
}

/// Display-cell offset of `char_offset` into `text`, used to report the match
/// cell's `offset/length(cell)` read-out.
pub fn cell_offset_into_value(text: &str, char_offset: usize) -> usize {
    display_width_char_prefix(text, char_offset.min(text.chars().count()))
}

/// Label for the search scope: `col:{name}` when scoped to one column, else
/// `all columns`. Mirrors the original dbm's `results_search_scope_label`.
pub fn search_scope_label(scope_column: Option<usize>, scope_column_name: Option<&str>) -> String {
    if let Some(col) = scope_column {
        let name = scope_column_name
            .map(str::to_string)
            .unwrap_or_else(|| col.to_string());
        format!("col:{name}")
    } else {
        "all columns".to_string()
    }
}

/// The read-out shown alongside the bottom-border search. While typing with no
/// matches it reports `scope: ...`; once a query is applied it reports
/// `count(scope): i/total` and, for the current match, `offset/length(cell): off/len`.
/// See [`super::state::ListState::search_title_extra`].
pub fn search_title_extra(
    search: &PaneSearch,
    match_index: usize,
    match_total: usize,
    scope_label: &str,
    match_cell: Option<(usize, usize)>,
) -> String {
    const SEP: &str = "   ";
    let mut parts = Vec::new();
    if search.active || search.has_filter() {
        if search.has_filter() {
            if match_total > 0 {
                parts.push(format!(
                    "count({scope_label}): {}/{}",
                    match_index + 1,
                    match_total
                ));
            } else {
                parts.push(format!("count({scope_label}): 0"));
            }
        } else {
            parts.push(format!("scope: {scope_label}"));
        }
    }
    if let Some((offset, len)) = match_cell.filter(|_| match_total > 0 && search.has_filter()) {
        parts.push(format!("offset/length(cell): {offset}/{len}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("{SEP}{}", parts.join(SEP))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::view::format::{results_col_text_view, results_table_width};
    use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;
    use ratatui::style::Color;

    fn col(name: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_name: "text".into(),
            type_display: "text".into(),
            comment: None,
        }
    }

    fn sample() -> QueryResultData {
        QueryResultData {
            columns: vec![col("id"), col("name")],
            rows: vec![
                vec!["1".into(), "alpha".into()],
                vec!["2".into(), "beta".into()],
                vec!["3".into(), "gamma".into()],
            ],
            rows_affected: None,
            total_rows: None,
        }
    }

    #[test]
    fn find_matches_all_columns() {
        let r = sample();
        // "a" appears only in column 1: "alpha"(0,4) x2, "beta"(4) x1, and
        // "gamma"(1,4) x2 = 5 occurrences across all columns.
        let hits = find_matches(&r, "a", None, TextSearchOptions::default());
        assert_eq!(hits.len(), 5);
        assert!(hits.iter().all(|m| m.col == 1));
    }

    #[test]
    fn find_matches_scoped_to_column() {
        let r = sample();
        let hits = find_matches(&r, "bet", Some(1), TextSearchOptions::default());
        assert!(hits.iter().all(|m| m.col == 1));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].row, 1);
    }

    #[test]
    fn find_matches_case_sensitive_by_default() {
        let r = sample();
        assert_eq!(
            find_matches(&r, "BET", Some(1), TextSearchOptions::default()).len(),
            0
        );
    }

    #[test]
    fn find_matches_ignore_case_option() {
        let r = sample();
        let opts = TextSearchOptions { ignore_case: true };
        assert_eq!(find_matches(&r, "BET", Some(1), opts).len(), 1);
    }

    #[test]
    fn match_starts_in_cell_lists_occurrences() {
        // "alpha": 'a' at char offsets 0 and 4.
        let matches = vec![
            ResultsSearchMatch {
                row: 0,
                col: 1,
                start: 0,
            },
            ResultsSearchMatch {
                row: 0,
                col: 1,
                start: 4,
            },
            ResultsSearchMatch {
                row: 1,
                col: 1,
                start: 1,
            },
        ];
        assert_eq!(match_starts_in_cell(&matches, 0, 1), vec![0, 4]);
        assert_eq!(match_starts_in_cell(&matches, 1, 1), vec![1]);
        assert!(match_starts_in_cell(&matches, 2, 1).is_empty());
    }

    #[test]
    fn highlight_line_colours_match_and_keeps_width() {
        // Wide enough columns that "alpha beta" fully fits the visible window.
        let widths = vec![12u16, 12];
        let tv = results_col_text_view(1, &widths, results_table_width(&widths), 0).unwrap();
        // "alpha beta": 'a' at char offsets 0 and 4; current match is offset 0.
        let current = Style::default().fg(Color::Blue).bg(Color::Yellow);
        let other = Style::default().fg(Color::Yellow);
        let line = cell_highlight_line(
            "alpha beta",
            0,
            tv.text_w,
            "a",
            &[0, 4],
            Some(0),
            Style::default(),
            other,
            current,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(
            text, "alpha beta",
            "highlight must not change the visible text"
        );

        // Segments: [0,1) current match, [1,4) plain, [4,5) other match, [5,10) plain.
        assert_eq!(line.spans.len(), 4);
        assert_eq!(line.spans[0].content.as_ref(), "a");
        assert_eq!(line.spans[1].content.as_ref(), "lph");
        assert_eq!(line.spans[2].content.as_ref(), "a");
        assert_eq!(line.spans[3].content.as_ref(), " beta");
        // First span uses the current-match style passed in (accent fill).
        assert_eq!(line.spans[0].style.bg, Some(Color::Yellow));
        // The other match uses the passed-in other-match accent foreground.
        assert_eq!(line.spans[2].style.fg, Some(Color::Yellow));
    }

    #[test]
    fn cell_highlight_uses_passed_match_styles() {
        // "banana": "an" matches at char offsets 1 and 3.
        let current = Style::default().bg(Color::Magenta);
        let other = Style::default().fg(Color::Red);
        // No active match: every match takes the "other" accent style.
        let line = cell_highlight_line(
            "banana",
            0,
            20,
            "an",
            &[1, 3],
            None,
            Style::default(),
            other,
            current,
        );
        assert_eq!(line.spans.len(), 4);
        assert_eq!(line.spans[0].content.as_ref(), "b");
        assert_eq!(line.spans[1].content.as_ref(), "an");
        assert_eq!(line.spans[1].style.fg, Some(Color::Red));
        assert_eq!(line.spans[2].content.as_ref(), "an");
        assert_eq!(line.spans[2].style.fg, Some(Color::Red));
        assert_eq!(line.spans[3].content.as_ref(), "a");
        // Current style (magenta bg) is unused without an active match on this cell.
        assert_eq!(line.spans[0].style.bg, None);

        // When the second match is active, it carries the current style.
        let line = cell_highlight_line(
            "banana",
            0,
            20,
            "an",
            &[1, 3],
            Some(3),
            Style::default(),
            other,
            current,
        );
        assert_eq!(line.spans[2].content.as_ref(), "an");
        assert_eq!(line.spans[2].style.bg, Some(Color::Magenta));
    }

    #[test]
    fn search_scope_label_renders_column_name() {
        assert_eq!(search_scope_label(Some(1), Some("name")), "col:name");
        assert_eq!(search_scope_label(Some(0), Some("id")), "col:id");
        // Falls back to the bare index when the column name is unknown.
        assert_eq!(search_scope_label(Some(3), None), "col:3");
        assert_eq!(search_scope_label(None, None), "all columns");
    }

    #[test]
    fn search_title_extra_typing_with_no_matches_shows_scope() {
        let mut search = PaneSearch::default();
        search.start();
        let extra = search_title_extra(&search, 0, 0, "col:name", None);
        assert!(extra.contains("scope: col:name"));
        assert!(!extra.contains("count("));
    }

    #[test]
    fn search_title_extra_applied_shows_count_and_offset() {
        let search = PaneSearch {
            query: "a".into(),
            active: false,
            options: TextSearchOptions::default(),
        };
        let extra = search_title_extra(&search, 1, 5, "col:name", Some((3, 12)));
        assert!(extra.contains("count(col:name): 2/5"));
        assert!(extra.contains("offset/length(cell): 3/12"));
    }

    #[test]
    fn search_title_extra_all_columns_and_zero_matches() {
        let zero = PaneSearch {
            query: "zzz".into(),
            active: false,
            options: TextSearchOptions::default(),
        };
        let extra = search_title_extra(&zero, 0, 0, "col:id", None);
        assert!(extra.contains("count(col:id): 0"));
        assert!(!extra.contains("offset/length"));

        let seen = PaneSearch {
            query: "a".into(),
            active: false,
            options: TextSearchOptions::default(),
        };
        let global = search_title_extra(&seen, 0, 5, "all columns", Some((12, 48)));
        assert!(global.contains("count(all columns): 1/5"));
        assert!(global.contains("offset/length(cell): 12/48"));
    }
}
