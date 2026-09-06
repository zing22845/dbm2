//! History list geometry: the shared viewport calculation, row hit-testing
//! and the list area split. Used by both the renderer and the click
//! hit-tester, so the row you hit is always the row that was drawn.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::common::components::line_numbers;
use crate::common::view::pane_scrollbar::{
    PaneScrollLayout, RowHeights, pane_anchor, pane_scroll_layout,
};

use super::super::splitter::state::clamp_detail_pane_width;
use super::super::store::SqlHistoryStore;
use super::state::ListState;

/// Shared viewport computation used by both the renderer and [`row_hit_at`].
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

    // Unified viewport anchor (height-aware; uniform for this non-wrapping list).
    let anchor = pane_anchor(
        total,
        RowHeights::Uniform(1),
        viewport,
        state.v_scroll,
        cursor,
        state.scroll_locked,
    );
    let start = anchor.start;
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
        let clamped_detail = clamp_detail_pane_width(detail_w).min(inner.width.saturating_sub(2));
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
            .constraints([Constraint::Min(1), Constraint::Length(list_footer_height)])
            .split(body_w[2]);
        list_col[0]
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(list_footer_height)])
            .split(inner);
        chunks[0]
    }
}

fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x.saturating_add(r.width) && y >= r.y && y < r.y.saturating_add(r.height)
}
