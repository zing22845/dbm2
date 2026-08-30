//! Discovery results feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::pane_scrollbar::{
    PaneScrollLayout, draw_vertical_pane_scrollbar, pane_scroll_layout,
};
use crate::common::view::theme::Theme;

use super::state::ResultsState;

/// Result of [`compute_results_viewport`]: the shared viewport calculation
/// used by [`render`], [`hit_test`], and [`v_scrollbar_hit`].
pub struct DiscoverResultsViewport {
    pub body: Rect,
    pub layout: PaneScrollLayout,
    pub content: Rect,
    /// Visible data rows (content.height — results has no header row).
    pub viewport: usize,
    /// Cursor-anchored v_scroll (first visible data row index, in filtered space).
    pub start: usize,
    /// Total visible rows (after filter).
    pub total: usize,
    pub max_scroll: usize,
}

/// Shared body-area computation. Discover results has no Table header, so
/// every content row IS a data row — even simpler than discover targets.
fn compute_results_body(area: Rect, _state: &ResultsState) -> Option<Rect> {
    use crate::common::utils::text_width::wrapped_line_count;
    use crate::common::view::hints::discover_results_footer_text;

    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(area);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    let footer_text = discover_results_footer_text();
    let footer_h = if footer_text.is_empty() {
        0
    } else {
        wrapped_line_count(&footer_text, inner.width).max(1)
    };

    let body = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer_h),
    );
    if body.width == 0 || body.height == 0 {
        return None;
    }
    Some(body)
}

/// Compute the discover results viewport. One source of truth for render,
/// hit_test, and v_scrollbar_hit. Encodes pane_scroll_layout + discover-style
/// cursor anchoring (skipped when `scroll_locked`).
pub fn compute_results_viewport(
    area: Rect,
    state: &ResultsState,
) -> Option<DiscoverResultsViewport> {
    let total = state.row_count();
    if total == 0 {
        return None;
    }
    let body = compute_results_body(area, state)?;
    let viewport_rows = body.height as usize;
    let layout = pane_scroll_layout(body, body.width, total, viewport_rows);
    let content = layout.content_area;

    // No Table header — every content row IS a data row.
    let viewport = content.height.max(1) as usize;
    let max_scroll = total.saturating_sub(viewport);

    // Discover-style anchor via shared helper.
    let start = crate::common::view::pane_scrollbar::discover_anchor(
        state.scroll,
        max_scroll,
        state.cursor,
        viewport,
        state.scroll_locked,
    );

    Some(DiscoverResultsViewport {
        body,
        layout,
        content,
        viewport,
        start,
        total,
        max_scroll,
    })
}

use crate::common::view::pane_scrollbar::ScrollbarHitInfo;

/// Hit-test the discover results pane's vertical scrollbar — delegates to shared helper.
pub fn v_scrollbar_hit(
    area: Rect,
    state: &ResultsState,
    x: u16,
    y: u16,
) -> Option<ScrollbarHitInfo> {
    let rv = compute_results_viewport(area, state)?;
    crate::common::view::pane_scrollbar::v_scrollbar_hit(&rv.layout, rv.max_scroll, x, y)
}

/// Hit-test the discover results list content area. Returns the row index in
/// **filtered (visible)** space if `(x, y)` lands on a data row, or `None`
/// otherwise (scrollbar, footer, outside block, etc.).
pub fn hit_test(area: Rect, state: &ResultsState, x: u16, y: u16) -> Option<usize> {
    let rv = compute_results_viewport(area, state)?;
    let content = rv.content;

    if x < content.x
        || x >= content.x + content.width
        || y < content.y
        || y >= content.y + content.height
    {
        return None;
    }

    let row_in_content = (y - content.y) as usize;
    let data_row = row_in_content.min(rv.viewport.saturating_sub(1));
    let row_idx = rv.start + data_row;
    if row_idx < rv.total {
        Some(row_idx)
    } else {
        None
    }
}

/// Render the results list of discovered instances. The border highlights only
/// when the results pane owns focus, and a footer line shows the results keys.
///
/// Now uses pane_scroll_layout (v_scrollbar reservation + discover-style
/// cursor anchor) — same pattern as history/results and discover targets.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ResultsState,
    focus: crate::app_shell::nav::DiscoverPane,
    layout_out: &std::cell::RefCell<Option<usize>>,
) {
    use crate::common::view::hints::{discover_results_footer_text, draw_pane_footer};
    let p = theme.palette();
    let focused = focus == crate::app_shell::nav::DiscoverPane::Results;

    let block = Block::default()
        .title(" results ")
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let footer_text = discover_results_footer_text();

    // ---- SHARED VIEWPORT CALCULATION ----
    let rv = match compute_results_viewport(area, state) {
        Some(v) => v,
        None => {
            // Empty list: still reserve scrollbar area for consistent block width.
            if !footer_text.is_empty() {
                let body = compute_results_body(area, state);
                if let Some(body) = body {
                    let footer_h = inner.height.saturating_sub(body.height);
                    if footer_h > 0 {
                        let footer_area = Rect::new(inner.x, inner.y + body.height, inner.width, footer_h);
                        draw_pane_footer(frame, theme, footer_area, &footer_text);
                    }
                }
            }
            *layout_out.borrow_mut() = Some(0);
            return;
        }
    };

    let content = rv.content;
    let layout = &rv.layout;
    let start = rv.start;
    let viewport = rv.viewport;
    let _ = rv.total;

    // Write back scroll_offset so the shell can sync state on next frame.
    *layout_out.borrow_mut() = Some(start);

    // Render data rows as Paragraph lines inside the scrollbar-reserved content.
    let mut lines = Vec::new();
    let visible = state.visible_indices();
    for (vis, &idx) in visible.iter().enumerate().skip(start).take(viewport) {
        let item = &state.items[idx];
        let row_focused = vis == state.cursor;
        let checked = result_selection_glyph(item.already_registered, state.selected.contains(&idx));
        let style = if row_focused {
            Style::default()
                .fg(p.selection_text)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg)
        };
        let line_style = if row_focused {
            Style::default().bg(p.selection_bg)
        } else {
            Style::default()
        };
        lines.push(
            Line::from(vec![
                Span::styled(format!("[{checked}]"), Style::default().fg(p.accent)),
                Span::styled(format!("  {}:{}", item.host, item.port), style),
                Span::styled(
                    format!("  ({})", item.confidence.as_str()),
                    Style::default().fg(p.muted),
                ),
            ])
            .style(line_style),
        );
    }
    frame.render_widget(Paragraph::new(lines), content);

    // Vertical scrollbar — only when the list actually overflows.
    if let Some(bar) = layout.v_scrollbar {
        draw_vertical_pane_scrollbar(
            frame,
            bar,
            start,
            viewport,
            rv.max_scroll,
            p,
            false,
        );
    }

    // Footer area — drawn in the space between the block's inner bottom and
    // the body we computed earlier.
    let body = rv.body;
    let footer_h = inner.height.saturating_sub(body.height);
    if footer_h > 0 {
        let footer_area = Rect::new(inner.x, inner.y + body.height, inner.width, footer_h);
        draw_pane_footer(frame, theme, footer_area, &footer_text);
    }
}

/// The row mark for a result, matching the original dbm: an already-registered
/// instance is `×`, a selected (unregistered) one is `✓`, and an unselected
/// unregistered row carries no mark at all.
pub fn result_selection_glyph(already_registered: bool, selected: bool) -> &'static str {
    if already_registered {
        "×"
    } else if selected {
        "✓"
    } else {
        " "
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_selection_glyph_three_states() {
        assert_eq!(result_selection_glyph(false, false), " ");
        assert_eq!(result_selection_glyph(false, true), "✓");
        assert_eq!(result_selection_glyph(true, false), "×");
        assert_eq!(result_selection_glyph(true, true), "×");
    }
}
