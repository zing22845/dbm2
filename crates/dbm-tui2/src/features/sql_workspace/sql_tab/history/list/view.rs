//! History list sub-feature view.
//!
//! Renders the scrollable list of SQL history entries with a fixed
//! row-number gutter (DarkGray, edtui Absolute style) and per-row horizontal
//! scroll. Vertical scroll uses a discover-style viewport anchor: `v_scroll`
//! is the viewport start row; cursor only pushes the viewport when it would
//! fall outside the current window. The list is always drawn inside the
//! parent History border — it never renders its own outer border. Also
//! exposes hit-test helpers (`row_hit_at`, `compute_list_area`) that mirror
//! the renderer's split geometry so click handling can resolve row indices
//! consistently.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::common::components::line_numbers;
use crate::common::view::pane_scrollbar::{
    PaneScrollLayout, draw_horizontal_pane_scrollbar, draw_vertical_pane_scrollbar,
    pane_scroll_layout,
};
use crate::common::view::theme::Theme;

use super::state::ListState;
use super::super::splitter::state::clamp_detail_pane_width;
use super::super::store::{SqlHistoryStore, history_one_line};

/// Shared viewport computation used by both [`render`] and [`row_hit_at`].
/// Encodes gutter splitting, effective-layout (re-run pane_scroll_layout
/// when the h_scrollbar disappears), and discover-style cursor anchoring
/// — all in one place so the hit-test row index always matches what the
/// renderer drew.
pub struct HistoryViewport {
    /// Layout with the correct content_area + v/h scrollbar rects.
    pub effective_layout: PaneScrollLayout,
    /// Gutter rect (0-width when no room). Only needed by the renderer.
    pub gutter_rect: Rect,
    /// Content rows visible in the viewport.
    pub viewport: usize,
    /// Anchored v_scroll (viewport start row index).
    pub start: usize,
    /// `start + viewport` clamped to the visible range.
    pub end: usize,
    /// Total number of visible entries.
    pub total: usize,
}

/// Compute the history list viewport given the full list area, state, the
/// selected-entry's display width (for h_scrollbar detection), the visible
/// indices, and cursor position. Returns `None` when the list is empty.
pub fn compute_history_viewport(
    list_area: Rect,
    state: &ListState,
    selected_width: usize,
    visible: &[usize],
    cursor: usize,
) -> Option<HistoryViewport> {
    if visible.is_empty() {
        return None;
    }

    // --- Split list_area into [gutter | inner_content] -----------------------
    let gutter_w = line_numbers::gutter_width(visible.len());
    let (gutter_rect, inner_content) = if list_area.width > gutter_w {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(gutter_w), Constraint::Min(1)])
            .split(list_area);
        (chunks[0], chunks[1])
    } else {
        (Rect::default(), list_area)
    };

    let viewport_rows = inner_content.height as usize;
    let total = visible.len();

    // --- effective_layout: re-run if h_scrollbar is not needed -----------
    let layout = pane_scroll_layout(inner_content, selected_width as u16, total, viewport_rows);
    let content_w = layout.content_area.width as usize;
    let needs_h = selected_width > content_w;
    let effective_layout = if needs_h {
        layout
    } else {
        pane_scroll_layout(inner_content, 0, total, viewport_rows)
    };

    let content = effective_layout.content_area;
    let viewport = content.height.max(1) as usize;

    // Discover-style cursor anchor via shared helper.
    let max_scroll = total.saturating_sub(viewport);
    let start = crate::common::view::pane_scrollbar::discover_anchor(
        state.v_scroll,
        max_scroll,
        cursor,
        viewport,
        state.scroll_locked,
    );
    let end = (start + viewport).min(total);

    Some(HistoryViewport {
        effective_layout,
        gutter_rect,
        viewport,
        start,
        end,
        total,
    })
}

/// Render the list rows (and its scrollbar) into `area`. Used inside the
/// shared History border; the list does not draw its own border.
///
/// Returns the reconciled viewport start row — the caller feeds it back to
/// state after the terminal.draw closure (discover-style layout_out pattern).
#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    list_area: Rect,
    state: &ListState,
    entries: &[String],
    visible: &[usize],
    cursor: usize,
    _focused: bool,
) -> Option<usize> {
    let p = theme.palette();
    if visible.is_empty() {
        let hint = if state.search.has_filter() {
            "No matching history"
        } else {
            "No history yet"
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                hint,
                Style::default().fg(p.muted),
            ))),
            list_area,
        );
        return None;
    }

    // Compute selected entry's display width for h_scrollbar detection.
    let selected_idx = visible.get(cursor).copied().and_then(|idx| entries.get(idx));
    let selected_width = selected_idx
        .map(|sql| super::super::store::history_line_display_width(sql) as usize)
        .unwrap_or(0);

    // ---- SHARED VIEWPORT CALCULATION ----
    let hv = compute_history_viewport(list_area, state, selected_width, visible, cursor)?;
    let gutter_rect = hv.gutter_rect;
    let gutter_w = gutter_rect.width;
    let content = hv.effective_layout.content_area;
    let effective_layout = &hv.effective_layout;
    let viewport = hv.viewport;
    let start = hv.start;
    let end = hv.end;

    let selected_h_scroll = state.h_scroll;
    let reconciled_start = hv.start;

    // --- Gutter lines (fixed, no horizontal scroll) --------------------------
    let gutter_style = line_numbers::gutter_style(); // DarkGray
    let mut gutter_lines: Vec<Line> = Vec::new();
    for (row, _) in visible[start..end].iter().enumerate() {
        let row_num = start + row + 1;
        let selected = start + row == cursor;
        let num_span = if selected {
            Span::styled(
                line_numbers::format_gutter(row_num, gutter_w),
                Style::default()
                    .fg(p.selection_text)
                    .bg(p.selection_bg),
            )
        } else {
            Span::styled(line_numbers::format_gutter(row_num, gutter_w), gutter_style)
        };
        gutter_lines.push(Line::from(num_span));
    }
    if !gutter_rect.is_empty() {
        frame.render_widget(Paragraph::new(gutter_lines), gutter_rect);
    }

    // --- SQL content lines (horizontal scroll lives here) --------------------
    let lines: Vec<Line> = visible[start..end]
        .iter()
        .enumerate()
        .map(|(row, &idx)| {
            let sql = &entries[idx];
            let selected = start + row == cursor;
            let line_text = history_one_line(sql);
            let line_width = UnicodeWidthStr::width(line_text);

            let row_h_scroll = if selected { selected_h_scroll } else { 0 };

            let content_w = content.width as usize;
            let max_scroll = line_width.saturating_sub(content_w);
            let scroll = row_h_scroll.min(max_scroll);
            let right_overflow = scroll + content_w < line_width;
            let right_label_w = if right_overflow { 3 } else { 0 };
            let text_budget = content_w.saturating_sub(right_label_w);

            let shown_text = if scroll > 0 {
                skip_display(line_text, scroll)
            } else {
                line_text.to_string()
            };
            let shown_text = truncate_display(&shown_text, text_budget, 0);

            let right_label = if right_overflow { "..." } else { "" };
            let display_text = format!("{shown_text}{right_label}");

            let style = if selected {
                Style::default()
                    .fg(p.selection_text)
                    .bg(p.selection_bg)
            } else {
                Style::default().fg(p.fg)
            };
            Line::from(Span::styled(display_text, style))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), content);

    if let Some(bar) = effective_layout.v_scrollbar {
        let max_scroll = visible.len().saturating_sub(viewport);
        draw_vertical_pane_scrollbar(
            frame,
            bar,
            start,
            viewport,
            max_scroll,
            p,
            false,
        );
    }

    if let Some(bar) = effective_layout.h_scrollbar {
        let max_h = selected_width.saturating_sub(content.width as usize);
        let thumb_pos = selected_h_scroll.min(max_h);
        draw_horizontal_pane_scrollbar(
            frame,
            bar,
            thumb_pos,
            content.width as usize,
            max_h,
            p,
            false,
        );
    }

    Some(reconciled_start)
}

/// Hit-test: given the history pane's `inner` area, compute which visible
/// row index was clicked at `(x, y)`. Returns `None` when the click is
/// outside the list content (scrollbar, footer, detail pane, etc.).
#[allow(clippy::too_many_arguments)]
pub fn row_hit_at(
    inner: Rect,
    state: &ListState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
    x: u16,
    y: u16,
    detail_visible: bool,
    detail_w: u16,
    list_footer_height: u16,
) -> Option<usize> {
    let visible = state.visible_indices(store, instance, connection);
    if visible.is_empty() {
        return None;
    }

    let list_area = compute_list_area(inner, detail_visible, detail_w, list_footer_height);
    if !contains(list_area, x, y) {
        return None;
    }

    // ---- SHARED VIEWPORT CALCULATION ----
    // Same function render uses — so gutter split, effective_layout (h_scrollbar
    // eating a row), discover-style anchor (with scroll_locked guard) are all
    // identical between what we draw and what we hit-test.
    let entries = store.entries(instance, connection);
    let cursor = state.cursor.min(visible.len().saturating_sub(1));
    let selected_width = visible
        .get(cursor)
        .copied()
        .and_then(|idx| entries.get(idx))
        .map(|sql| super::super::store::history_line_display_width(sql) as usize)
        .unwrap_or(0);

    let hv = compute_history_viewport(list_area, state, selected_width, &visible, cursor)?;
    let content = hv.effective_layout.content_area;
    if !contains(content, x, y) {
        return None;
    }

    let y_offset = y.saturating_sub(content.y) as usize;
    let row_in_viewport = y_offset.min(hv.viewport.saturating_sub(1));
    let visible_idx = hv.start + row_in_viewport;

    if visible_idx < visible.len() {
        Some(visible_idx)
    } else {
        None
    }
}

/// Compute the list area (content + optional footer) inside the History
/// pane's `inner` rect, mirroring the renderer's split logic.
pub fn compute_list_area(
    inner: Rect,
    detail_visible: bool,
    detail_w: u16,
    list_footer_height: u16,
) -> Rect {
    if detail_visible {
        let clamped_detail = clamp_detail_pane_width(detail_w)
            .min(inner.width.saturating_sub(2));
        let body_w = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(clamped_detail),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(inner);
        let list_col = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(list_footer_height),
            ])
            .split(body_w[2]);
        list_col[0]
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(list_footer_height),
            ])
            .split(inner);
        chunks[0]
    }
}

fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x.saturating_add(r.width) && y >= r.y && y < r.y.saturating_add(r.height)
}

/// Truncate a display string to `avail` width cells, handling the prefix offset.
pub(crate) fn truncate_display(text: &str, avail: usize, prefix_width: usize) -> String {
    let total_avail = avail.saturating_sub(prefix_width);
    if total_avail == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let w = UnicodeWidthStr::width(ch.to_string().as_str());
        if width + w > total_avail {
            break;
        }
        result.push(ch);
        width += w;
    }
    result
}

/// Skip `n` characters from the start of the text (for horizontal scroll).
pub(crate) fn skip_display(text: &str, skip: usize) -> String {
    if skip == 0 {
        return text.to_string();
    }
    let mut result = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let w = UnicodeWidthStr::width(ch.to_string().as_str());
        if width >= skip {
            result.push(ch);
        }
        width += w;
    }
    result
}
