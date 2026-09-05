//! History detail sub-feature view.
//!
//! Shows the full SQL of the selected history entry with line numbers,
//! search-match highlighting, and independent vertical scroll. Owns the
//! themed renderer (`draw_history_detail`) plus pure helpers for line
//! counting, wrapping, and search-to-scroll mapping.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::common::components::line_numbers;
use crate::common::components::search::{TextSearchOptions, find_match_starts};
use crate::common::utils::text_width;
use crate::common::view::theme::Theme;

use super::state::DetailState;

pub fn sql_line_count(sql: &str) -> usize {
    if sql.is_empty() {
        0
    } else {
        sql.lines().count()
    }
}

/// Text area width inside the Detail block (borders excluded).
pub fn detail_text_width(pane_width: u16) -> u16 {
    pane_width.saturating_sub(2).max(1)
}

pub(crate) fn wrapped_row_count(line: &str, width: usize) -> usize {
    if line.is_empty() {
        return 1;
    }
    let mut rows = 1usize;
    let mut used = 0usize;
    for ch in line.chars() {
        let cw = text_width::char_width(ch);
        if used > 0 && used + cw > width {
            rows += 1;
            used = cw;
        } else {
            used += cw;
        }
    }
    rows
}

/// Display rows when the content width is already known (inside borders).
pub fn detail_display_line_count_at(text_width_after: u16, sql: &str) -> usize {
    if sql.is_empty() {
        return 0;
    }
    let logical = sql.lines().count().max(1);
    let w = line_numbers::text_width_after_gutter(text_width_after, logical) as usize;
    sql.lines().map(|line| wrapped_row_count(line, w)).sum()
}

/// Clamp the detail scroll to the total wrapped display rows minus viewport.
pub fn clamp_detail_scroll(
    state: &mut DetailState,
    sql: &str,
    text_width_after: u16,
    viewport_lines: usize,
) {
    let max =
        detail_display_line_count_at(text_width_after, sql).saturating_sub(viewport_lines.max(1));
    state.scroll = state.scroll.min(max);
}

pub fn detail_line_count_label(sql: &str) -> String {
    let lines = sql_line_count(sql);
    if lines <= 1 {
        "1 line".to_string()
    } else {
        format!("{lines} lines")
    }
}

pub fn scroll_half_page(
    state: &mut DetailState,
    sql: &str,
    text_width_after: u16,
    viewport_lines: usize,
    down: bool,
) {
    if viewport_lines == 0 {
        return;
    }
    let total = detail_display_line_count_at(text_width_after, sql).max(1);
    let half = (viewport_lines / 2).max(1);
    let max_scroll = total.saturating_sub(viewport_lines.max(1));
    if down {
        state.scroll = (state.scroll + half).min(max_scroll);
    } else {
        state.scroll = state.scroll.saturating_sub(half);
    }
}

/// Line index of the first line containing a search match, if any.
pub fn first_match_line(sql: &str, query: &str, opts: TextSearchOptions) -> Option<usize> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    for (idx, line) in sql.lines().enumerate() {
        if !find_match_starts(line, query, opts).is_empty() {
            return Some(idx);
        }
    }
    None
}

/// Display row of the first wrapped line that contains a search match.
pub fn first_match_display_line(
    sql: &str,
    query: &str,
    opts: TextSearchOptions,
    text_width_after: u16,
) -> Option<usize> {
    let logical = first_match_line(sql, query, opts)?;
    let w = text_width_after.max(1) as usize;
    let mut display = 0usize;
    for (idx, line) in sql.lines().enumerate() {
        if idx == logical {
            return Some(display);
        }
        display += wrapped_row_count(line, w);
    }
    Some(display)
}

fn highlight_style(current: bool, theme: &Theme) -> Style {
    let p = theme.palette();
    if current {
        p.current_match_style()
    } else {
        p.match_style()
    }
}

fn line_to_spans(
    line: &str,
    query: &str,
    opts: TextSearchOptions,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let query = query.trim();
    if query.is_empty() {
        return vec![Span::raw(line.to_string())];
    }
    let starts = find_match_starts(line, query, opts);
    if starts.is_empty() {
        return vec![Span::raw(line.to_string())];
    }
    let chars: Vec<char> = line.chars().collect();
    let query_len = query.chars().count();
    let mut spans = Vec::new();
    let mut last = 0usize;
    for (i, &start) in starts.iter().enumerate() {
        if start > last {
            spans.push(Span::raw(chars[last..start].iter().collect::<String>()));
        }
        let end = (start + query_len).min(chars.len());
        spans.push(Span::styled(
            chars[start..end].iter().collect::<String>(),
            highlight_style(i == 0, theme),
        ));
        last = end;
    }
    if last < chars.len() {
        spans.push(Span::raw(chars[last..].iter().collect::<String>()));
    }
    spans
}

fn push_styled_char(spans: &mut Vec<Span<'static>>, ch: char, style: Style) {
    let piece = ch.to_string();
    if let Some(last) = spans.last_mut()
        && last.style == style
    {
        last.content = format!("{}{}", last.content, piece).into();
        return;
    }
    spans.push(if style == Style::default() {
        Span::raw(piece)
    } else {
        Span::styled(piece, style)
    });
}

fn wrap_spans_to_lines(spans: Vec<Span<'static>>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from(spans)];
    }
    let mut rows = Vec::new();
    let mut row_spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;

    for span in spans {
        let style = span.style;
        let content = span.content.to_string();
        for ch in content.chars() {
            let cw = text_width::char_width(ch);
            if used > 0 && used + cw > width {
                rows.push(if row_spans.is_empty() {
                    Line::from("")
                } else {
                    Line::from(std::mem::take(&mut row_spans))
                });
                used = 0;
            }
            push_styled_char(&mut row_spans, ch, style);
            used += cw;
        }
    }
    rows.push(if row_spans.is_empty() {
        Line::from("")
    } else {
        Line::from(row_spans)
    });
    rows
}

pub(crate) fn build_detail_display_lines(
    sql: &str,
    text_width_after: u16,
    query: &str,
    opts: TextSearchOptions,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if sql.is_empty() {
        return Vec::new();
    }
    let logical: Vec<&str> = sql.lines().collect();
    let gutter_w = line_numbers::gutter_width(logical.len().max(1));
    let w = text_width_after.saturating_sub(gutter_w).max(1) as usize;
    let mut out = Vec::new();
    for (i, line) in logical.iter().enumerate() {
        let wrapped = wrap_spans_to_lines(line_to_spans(line, query, opts, theme), w);
        out.extend(line_numbers::prefix_wrapped_line(i + 1, gutter_w, wrapped));
    }
    out
}

/// Draw the history detail pane. Fills `out_content_rect` / `out_v_scrollbar_rect`
/// so the caller can lay out a splitter / scrollbar.
#[allow(clippy::too_many_arguments)]
pub fn draw_history_detail(
    frame: &mut Frame,
    area: Rect,
    sql: &str,
    state: &DetailState,
    search: &crate::common::components::search::PaneSearch,
    theme: &Theme,
    out_content_rect: &mut Rect,
    out_v_scrollbar_rect: &mut Rect,
) {
    *out_content_rect = Rect::default();
    *out_v_scrollbar_rect = Rect::default();
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = theme.palette();

    let line_count = detail_display_line_count_at(area.width, sql);
    let needs_v = line_count > area.height as usize;
    let (text_area, v_bar) = if needs_v {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);
        (chunks[0], Some(chunks[1]))
    } else {
        (area, None)
    };
    *out_content_rect = text_area;

    let opts = search.options;
    let query = search.query.as_str();
    let viewport = text_area.height as usize;
    let display_lines = build_detail_display_lines(sql, text_area.width, query, opts, theme);
    let max_scroll = display_lines.len().saturating_sub(viewport.max(1));
    let scroll = state.scroll.min(max_scroll);

    let visible: Vec<Line> = display_lines
        .into_iter()
        .skip(scroll)
        .take(viewport)
        .collect();

    if visible.is_empty() {
        frame.render_widget(
            Paragraph::new("").style(Style::default().bg(p.surface)),
            text_area,
        );
    } else {
        frame.render_widget(Paragraph::new(visible), text_area);
    }

    if let Some(bar_area) = v_bar {
        *out_v_scrollbar_rect = bar_area;
        let max_scroll = line_count.saturating_sub(viewport.max(1)).max(1);
        let thumb = scroll.saturating_mul(bar_area.height as usize) / max_scroll;
        let thumb = thumb.min(bar_area.height as usize);
        let buf = frame.buffer_mut();
        for y in 0..bar_area.height {
            if y as usize == thumb {
                buf[(bar_area.x, bar_area.y + y)].set_style(Style::default().fg(p.accent));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::components::search::PaneSearch;

    #[test]
    fn first_match_line_finds_earliest_hit() {
        let sql = "SELECT 1\nFROM users\nWHERE id = 1";
        assert_eq!(
            first_match_line(sql, "FROM", TextSearchOptions::default()),
            Some(1)
        );
        assert!(first_match_line(sql, "missing", TextSearchOptions::default()).is_none());
    }

    #[test]
    fn scroll_on_selection_change_jumps_to_first_match() {
        let sql = "line0\nline1\nmatch here\nline3";
        let mut state = DetailState::default();
        let search = PaneSearch {
            query: "match".into(),
            ..Default::default()
        };
        crate::features::sql_workspace::sql_tab::history::detail::update::reconcile_on_selection_change(
            &mut state, sql, &search, 40, 2,
        );
        assert_eq!(state.scroll, 2);
    }

    #[test]
    fn scroll_on_selection_change_resets_without_filter() {
        let sql = "a\nb\nc";
        let mut state = DetailState {
            scroll: 3,
            pinned_sql: None,
        };
        let search = PaneSearch::default();
        crate::features::sql_workspace::sql_tab::history::detail::update::reconcile_on_selection_change(
            &mut state, sql, &search, 40, 2,
        );
        assert_eq!(state.scroll, 0);
    }

    #[test]
    fn long_logical_line_wraps_for_display_count() {
        let sql = "SELECT abcdefghijklmnopqrstuvwxyz FROM t";
        assert_eq!(detail_display_line_count_at(detail_text_width(42), sql), 2);
        assert_eq!(detail_display_line_count_at(detail_text_width(12), sql), 5);
    }

    #[test]
    fn scroll_half_page_respects_bounds() {
        let sql = "a\nb\nc\nd\ne\nf";
        let mut state = DetailState::default();
        scroll_half_page(&mut state, sql, 40, 2, true);
        assert_eq!(state.scroll, 1);
        scroll_half_page(&mut state, sql, 40, 2, true);
        assert_eq!(state.scroll, 2);
        for _ in 0..4 {
            scroll_half_page(&mut state, sql, 40, 2, true);
        }
        assert_eq!(state.scroll, 4);
    }
}
