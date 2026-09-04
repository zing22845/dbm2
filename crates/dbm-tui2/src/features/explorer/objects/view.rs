//! Explorer objects (object tree) feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::hints::{draw_pane_footer, footer_height, objects_pane_footer_text};
use crate::common::view::pane_scrollbar::{
    PaneScrollLayout, draw_horizontal_pane_scrollbar, draw_vertical_pane_scrollbar,
    pane_scroll_layout,
};
use crate::common::view::theme::Theme;

use super::state::ObjectsState;

/// Result of [`compute_objects_viewport`]: shared by render, row_at, toggle_at,
/// and v_scrollbar_hit so they agree on body geometry, viewport start, and
/// scrollbar placement.
pub struct ObjectsViewport {
    pub body: Rect,
    pub layout: PaneScrollLayout,
    pub content: Rect,
    pub viewport: usize,
    pub start: usize,
    pub total: usize,
    pub max_scroll: usize,
    /// Horizontal scroll max — `selected_row_width - content.width`. The
    /// single source of truth for Paragraph::scroll clamp, h_scrollbar
    /// thumb, and h_scrollbar_hit. All three read this field so they agree.
    pub max_h_scroll: usize,
}

/// Shared body-area computation used by render and row_at.
fn compute_objects_body(area: Rect, _state: &ObjectsState) -> Option<Rect> {
    let hint = objects_pane_footer_text();
    let footer_h =
        footer_height(&hint, area.width.saturating_sub(2)).min(area.height.saturating_sub(2));
    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(area);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }
    let body_h = inner.height.saturating_sub(footer_h);
    if body_h == 0 {
        return None;
    }
    Some(Rect::new(inner.x, inner.y, inner.width, body_h))
}

/// Compute the objects tree viewport. One source of truth for render, row_at,
/// toggle_at, and v_scrollbar_hit. Applies discover-style cursor anchoring
/// (skipped when `scroll_locked`).
pub fn compute_objects_viewport(
    area: Rect,
    state: &ObjectsState,
) -> Option<ObjectsViewport> {
    let total = state.rows.len();
    if total == 0 {
        return None;
    }
    let body = compute_objects_body(area, state)?;
    // Horizontal scrollbar shown only when the CURRENTLY SELECTED row overflows
    // (matching history list), not the widest row in the tree.
    let sel_row_w = state.selected_row_width();
    // First pass: reserve bars assuming the selected row might overflow.
    let first_pass = pane_scroll_layout(body, sel_row_w, total, body.height as usize);
    // Second pass (effective_layout): if the first pass's content_area is
    // already wide enough for the selected row, there's no need for an
    // h_scrollbar — drop it so we don't steal a row of viewport height.
    let content_w = first_pass.content_area.width;
    let (layout, max_h_scroll) = if sel_row_w > content_w {
        let max_h = sel_row_w.saturating_sub(content_w) as usize;
        (first_pass, max_h)
    } else {
        (
            pane_scroll_layout(body, 0, total, body.height as usize),
            0usize,
        )
    };
    let content = layout.content_area;

    let viewport = content.height.max(1) as usize;
    let max_scroll = total.saturating_sub(viewport);

    // Discover-style anchor via shared helper.
    let start = crate::common::view::pane_scrollbar::discover_anchor(
        state.scroll.get(),
        max_scroll,
        state.cursor,
        viewport,
        state.scroll_locked,
    );
    state.scroll.set(start);

    // Write the viewport-aware max back so update's ScrollHorizontal / SetHScroll
    // can clamp to the real upper bound and stay in sync.
    state.cached_h_max_scroll.set(max_h_scroll);

    Some(ObjectsViewport {
        body,
        layout,
        content,
        viewport,
        start,
        total,
        max_scroll,
        max_h_scroll,
    })
}

use crate::common::view::pane_scrollbar::ScrollbarHitInfo;

/// Hit-test the objects pane's vertical scrollbar — delegates to shared helper.
pub fn v_scrollbar_hit(
    area: Rect,
    state: &ObjectsState,
    x: u16,
    y: u16,
) -> Option<ScrollbarHitInfo> {
    let ov = compute_objects_viewport(area, state)?;
    crate::common::view::pane_scrollbar::v_scrollbar_hit(&ov.layout, ov.max_scroll, x, y)
}

/// Hit-test the objects pane's horizontal scrollbar — delegates to shared helper.
pub fn h_scrollbar_hit(
    area: Rect,
    state: &ObjectsState,
    x: u16,
    y: u16,
) -> Option<ScrollbarHitInfo> {
    let ov = compute_objects_viewport(area, state)?;
    crate::common::view::pane_scrollbar::h_scrollbar_hit(&ov.layout, ov.max_h_scroll, x, y)
}

/// Hit-test a click inside the objects tree area to a visible row (absolute,
/// including scroll offset), mirroring `render`'s body geometry. Returns `None`
/// when the click is on the border, title, footer, or beyond the row count.
pub fn row_at(area: Rect, state: &ObjectsState, y: u16) -> Option<usize> {
    let ov = compute_objects_viewport(area, state)?;
    let content = ov.content;
    if y < content.y || y >= content.y + content.height {
        return None;
    }
    let row_in_content = (y - content.y) as usize;
    let data_row = row_in_content.min(ov.viewport.saturating_sub(1));
    let row_idx = ov.start + data_row;
    if row_idx < ov.total {
        Some(row_idx)
    } else {
        None
    }
}

/// Like [`row_at`], but also require the click x to land on the row's
/// expand/collapse marker (`▸`/`▾`). Returns the visible row when the marker
/// was clicked, so the caller can toggle expansion. The marker column accounts
/// for the row's depth indentation and the horizontal scroll.
pub fn toggle_at(area: Rect, state: &ObjectsState, x: u16, y: u16) -> Option<usize> {
    let row = row_at(area, state, y)?;
    let depth = state.rows.get(row).map(|r| r.depth).unwrap_or(0) as u16;
    let ov = compute_objects_viewport(area, state)?;
    // Rows render as "{indent}{marker} label" with indent = 2 cols per depth
    // and NO leading space, so the marker sits at content.x + depth*2 (matching
    // `render`'s `format!("{indent}{marker} {label}")`). No +1 leading-space
    // offset here, unlike the instances pane (which does emit a leading space).
    // Horizontal scroll only moves the selected row, so subtract it only when
    // the clicked row IS the cursor row.
    let row_h_scroll = if row == state.cursor { state.h_scroll } else { 0 };
    let marker_col = ov
        .content
        .x
        .saturating_add(depth.saturating_mul(2))
        .saturating_sub(row_h_scroll);
    (x >= marker_col && x < marker_col.saturating_add(2)).then_some(row)
}

/// Render the object tree with indentation and expansion markers.
/// `region_focused` controls the border color so the shell focus is visible.
/// A pane footer hint occupies the bottom rows, wrapping to the pane width.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ObjectsState,
    region_focused: bool,
) {
    let p = theme.palette();

    let hint = objects_pane_footer_text();
    let footer_h = footer_height(&hint, area.width.saturating_sub(2)).min(area.height.saturating_sub(2));
    let block = Block::default()
        .title(" [O] Objects ")
        .borders(Borders::ALL)
        .border_style(p.child_border(region_focused));
    frame.render_widget(&block, area);
    let inner = block.inner(area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // ---- SHARED VIEWPORT CALCULATION ----
    let ov = match compute_objects_viewport(area, state) {
        Some(v) => v,
        None => {
            // Empty tree: still draw the footer so the pane looks consistent.
            if footer_h > 0 {
                let footer_area = Rect::new(
                    inner.x,
                    inner.y.saturating_add(inner.height.saturating_sub(footer_h)),
                    inner.width,
                    footer_h,
                );
                draw_pane_footer(frame, theme, footer_area, &hint);
            }
            return;
        }
    };

    let content = ov.content;
    let layout = &ov.layout;
    let start = ov.start;
    let viewport = ov.viewport;
    let body = ov.body;
    let viewport_w = layout.content_area.width as usize;
    let max_h = ov.max_h_scroll;
    let effective_h = state.h_scroll.min(max_h as u16);

    let mut lines = Vec::new();
    for idx in start..(start + viewport).min(ov.total) {
        let row = &state.rows[idx];
        let focused = idx == state.cursor;
        let style = if focused {
            Style::default()
                .fg(p.selection_text)
                .bg(p.selection_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg)
        };
        let marker = if row.expandable {
            if row.expanded { "▾" } else { "▸" }
        } else {
            "·"
        };
        let indent = "  ".repeat(row.depth);
        let full_text = format!("{indent}{marker} {}", row.label);
        // Per-row horizontal scroll: only the selected row scrolls.
        let row_h_scroll: usize = if focused { effective_h as usize } else { 0 };
        // Active rows reserve 1 cell for the trailing ● marker (tight against
        // the right border); inactive rows use the full width.
        let dot = "●";
        let text_max = if row.active { viewport_w.saturating_sub(1) } else { viewport_w };
        let display_text = crate::common::utils::text_width::truncate_from(
            &full_text,
            row_h_scroll,
            text_max,
        );
        if row.active && viewport_w >= 2 {
            let text_w = crate::common::utils::text_width::width(&display_text);
            let padding = text_max.saturating_sub(text_w);
            lines.push(Line::from(vec![
                Span::styled(display_text, style),
                Span::raw(" ".repeat(padding)),
                Span::styled(dot, Style::default().fg(p.success)),
            ]));
        } else {
            lines.push(Line::from(vec![Span::styled(display_text, style)]));
        }
    }
    if lines.is_empty() {
        let msg = if state.bound_connection.is_empty() {
            "Open a connection to browse objects"
        } else {
            "(no objects — press Enter on a connection to load the catalog)"
        };
        lines.push(Line::from(Span::styled(msg, Style::default().fg(p.muted))));
    }

    frame.render_widget(Paragraph::new(lines), content);

    if let Some(bar) = layout.h_scrollbar {
        draw_horizontal_pane_scrollbar(
            frame,
            bar,
            state.h_scroll as usize,
            viewport_w,
            max_h,
            p,
            false,
        );
    }

    // Vertical scrollbar — reserved by pane_scroll_layout above.
    if let Some(bar) = layout.v_scrollbar {
        draw_vertical_pane_scrollbar(
            frame,
            bar,
            start,
            viewport,
            ov.max_scroll,
            p,
            false,
        );
    }

    // Pane footer (inside the border).
    let footer_area = Rect::new(
        inner.x,
        body.y.saturating_add(body.height),
        inner.width,
        footer_h,
    );
    if footer_h > 0 {
        draw_pane_footer(frame, theme, footer_area, &hint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::explorer::objects::state::{ObjectsNode, ObjectsRow, ObjectsState};

    #[test]
    fn toggle_at_hits_the_marker_column_only() {
        let mut s = ObjectsState::default();
        s.rows = vec![ObjectsRow {
            depth: 0,
            expanded: false,
            expandable: true,
            active: false,
            node: ObjectsNode::Database { name: "db".into() },
            label: "db".into(),
        }];
        let area = Rect::new(0, 5, 40, 20);
        // depth 0 renders as "{marker} label", marker at body.x = area.x+1 = 1.
        assert_eq!(toggle_at(area, &s, 1, 6), Some(0), "marker column hits");
        // Clicking the border (x=0) or the label (x=3) is not the marker.
        assert_eq!(toggle_at(area, &s, 0, 6), None);
        assert_eq!(toggle_at(area, &s, 3, 6), None);
    }
}
