//! History feature rendering.
//!
//! The History feature owns two sub-panes: the **list** (searchable, applies a
//! statement) and the **detail** (preview of the selected statement). Mirrors
//! the original dbm: a single History border + title wraps BOTH sub-panes; when
//! the detail is visible the inner area is split into `[detail | splitter |
//! list]`, otherwise the list fills the whole pane. The list and detail do not
//! draw their own outer borders — the History pane provides it.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::common::components::line_numbers;
use crate::common::components::search::pane_search_title_line;
use crate::common::view::hints::{draw_footer, footer_height, history_list_footer_text};
use crate::common::view::pane_scrollbar::{
    draw_horizontal_pane_scrollbar, draw_vertical_pane_scrollbar, pane_scroll_layout,
};
use crate::common::view::theme::Theme;

use super::detail::draw_history_detail;
use super::splitter::state::clamp_detail_pane_width;
use super::state::HistoryState;
use super::store::{SqlHistoryStore, history_one_line};
use super::splitter::view as splitter_view;

/// Render the History pane. The single History border + title wraps the whole
/// `area` (both the list and, when `detail_visible`, the detail preview).
/// `detail_visible` is computed by the caller from the original dbm's
/// `detail_visible` condition (`workspace_pane == History` + has an entry);
/// `detail_w` is the width of the detail pane (clamped to its allowed range).
#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &HistoryState,
    store: &SqlHistoryStore,
    instance: &str,
    connection: &str,
    focused: bool,
    detail_visible: bool,
    detail_w: u16,
    splitter_hover: bool,
    splitter_drag: bool,
) -> Option<usize> {
    let search_active = state.search.text_input_active();
    let list_footer = history_list_footer_text(search_active, state.search.has_filter(), true);

    let p = theme.palette();
    let entries = store.entries(instance, connection);
    let visible = state.visible_indices(store, instance, connection);
    let cursor = state.cursor.min(visible.len().saturating_sub(1));

    // The footer width depends on which sub-pane it belongs to: when the detail
    // is visible each sub-pane carries its own footer inside the shared border.
    let detail_w = if detail_visible {
        clamp_detail_pane_width(detail_w)
    } else {
        0
    };
    let list_w = if detail_visible {
        area.width.saturating_sub(detail_w).saturating_sub(1) // detail + splitter
    } else {
        area.width
    };

    let title = pane_search_title_line(
        " [H] History",
        &state.search,
        true,
        false,
        Style::default().fg(p.muted),
        cursor,
        visible.len(),
        Some(area.width.saturating_sub(6)),
        None,
        Some(Style::default().fg(if focused { p.accent } else { p.muted })),
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    let (list_area, list_footer_area) = if detail_visible {
        // [detail | splitter | list] inside the shared border.
        let detail_w = clamp_detail_pane_width(detail_w).min(inner.width.saturating_sub(2));
        let body_h = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(detail_w),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(inner);
        tracing::debug!("history: split done inner={inner:?} detail={:?} list={:?}", body_h[0], body_h[2]);
        // Detail preview of the selected / pinned / first statement.
        if let Some(sql) = state
            .selected_entry(store, instance, connection)
            .or_else(|| state.detail.pinned_sql.clone())
            .or_else(|| store.entries(instance, connection).first().cloned())
        {
            let mut content = Rect::default();
            let mut v_bar = Rect::default();
            draw_history_detail(
                frame,
                body_h[0],
                &sql,
                &state.detail,
                &state.search,
                theme,
                &mut content,
                &mut v_bar,
            );
        }
        tracing::debug!("history: detail drawn");
        splitter_view::render(frame, body_h[1], splitter_hover, splitter_drag);
        // Each sub-pane's footer is inside the shared border, wrapped to its
        // own column width.
        let footer_h = footer_height(&list_footer, list_w.saturating_sub(2))
            .min(inner.height.saturating_sub(3));
        let list_col = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(footer_h)])
            .split(body_h[2]);
        (list_col[0], list_col[1])
    } else {
        let footer_h = footer_height(&list_footer, inner.width.saturating_sub(2))
            .min(inner.height.saturating_sub(3));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(footer_h)])
            .split(inner);
        (chunks[0], chunks[1])
    };

    let v_scroll_out = render_list_rows(frame, theme, list_area, state, entries, &visible, cursor, focused);

    // The list footer hints (inside the shared border).
    draw_footer(frame, theme, list_footer_area, &list_footer);

    v_scroll_out
}

/// Render the list rows (and its scrollbar) into `area`. Used inside the shared
/// History border; the list does not draw its own border.
#[allow(clippy::too_many_arguments)]
fn render_list_rows(
    frame: &mut Frame,
    theme: &Theme,
    list_area: Rect,
    state: &HistoryState,
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

    // --- Split list_area into [gutter | inner_content] -----------------------
    //
    // The gutter is a fixed left column displaying right-aligned row numbers
    // (edtui Absolute gutter style, DarkGray). It never scrolls — only the
    // inner_content to its right participates in horizontal scrolling.
    // This matches history detail / editor gutter behavior.
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

    // --- Horizontal scroll only considers SQL text width (no gutter) ----------
    let selected_idx = visible.get(cursor).copied().and_then(|idx| entries.get(idx));
    let selected_width = selected_idx
        .map(|sql| super::store::history_line_display_width(sql) as usize)
        .unwrap_or(0);

    let viewport_rows = inner_content.height as usize;
    let layout = pane_scroll_layout(inner_content, selected_width as u16, visible.len(), viewport_rows);
    let content = layout.content_area;
    let content_w = content.width as usize;
    let needs_h = selected_width > content_w;
    let effective_layout = if needs_h {
        layout
    } else {
        pane_scroll_layout(inner_content, 0, visible.len(), viewport_rows)
    };
    let content = effective_layout.content_area;
    let viewport = content.height.max(1) as usize;

    // Discover-style scroll anchor: v_scroll is the viewport start row. The
    // cursor only pushes the viewport when it would fall outside the current
    // window.
    let total = visible.len();
    let max_start = total.saturating_sub(viewport);
    let mut start = state.v_scroll.min(max_start);
    if cursor < start {
        start = cursor;
    } else if cursor >= start + viewport {
        start = cursor + 1 - viewport;
    }
    let end = (start + viewport).min(total);
    let reconciled_start = start;

    let selected_h_scroll = state.h_scroll;

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

            // Per-row h_scroll: selected row uses state.h_scroll, others stay at 0.
            let row_h_scroll = if selected { selected_h_scroll } else { 0 };

            let content_w = content.width as usize;
            // Clamp: the update layer returns a generous upper bound (≈
            // line_width), but the real max is `line_width - content_w`.
            let max_scroll = line_width.saturating_sub(content_w);
            let scroll = row_h_scroll.min(max_scroll);
            let right_overflow = scroll + content_w < line_width;
            let right_label_w = if right_overflow { 3 } else { 0 };
            let text_budget = content_w.saturating_sub(right_label_w);

            let shown_text = if scroll > 0 {
                // Scroll: skip left portion of the SQL text.
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

/// Truncate a display string to `avail` width cells, handling the prefix offset.
fn truncate_display(text: &str, avail: usize, prefix_width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
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
fn skip_display(text: &str, skip: usize) -> String {
    use unicode_width::UnicodeWidthStr;
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

/// Hit-test: given the history pane's `inner` area, compute which visible
/// row index was clicked at `(x, y)`. Returns `None` when the click is
/// outside the list content (scrollbar, footer, detail pane, etc.).
///
/// `detail_visible` and `detail_w` must match the rendering geometry so the
/// list area is computed identically.
#[allow(clippy::too_many_arguments)]
pub fn row_hit_at(
    inner: Rect,
    state: &HistoryState,
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

    let cursor = state.cursor.min(visible.len().saturating_sub(1));
    let viewport_rows = list_area.height as usize;
    let layout = crate::common::view::pane_scrollbar::pane_scroll_layout(
        list_area,
        list_area.width,
        visible.len(),
        viewport_rows.max(1),
    );
    let content = layout.content_area;
    if !contains(content, x, y) {
        return None;
    }

    let viewport = content.height.max(1) as usize;
    let start = cursor.saturating_sub(viewport / 2);
    let end = (start + viewport).min(visible.len());
    let start = end.saturating_sub(viewport);

    let y_offset = y.saturating_sub(content.y) as usize;
    let row_in_viewport = y_offset.min(viewport.saturating_sub(1));
    let visible_idx = start + row_in_viewport;

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
        let clamped_detail = super::splitter::state::clamp_detail_pane_width(detail_w)
            .min(inner.width.saturating_sub(2));
        let body_w = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([
                ratatui::layout::Constraint::Length(clamped_detail),
                ratatui::layout::Constraint::Length(1),
                ratatui::layout::Constraint::Min(0),
            ])
            .split(inner);
        let list_col = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Min(1),
                ratatui::layout::Constraint::Length(list_footer_height),
            ])
            .split(body_w[2]);
        list_col[0]
    } else {
        let chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Min(1),
                ratatui::layout::Constraint::Length(list_footer_height),
            ])
            .split(inner);
        chunks[0]
    }
}

fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x.saturating_add(r.width) && y >= r.y && y < r.y.saturating_add(r.height)
}
