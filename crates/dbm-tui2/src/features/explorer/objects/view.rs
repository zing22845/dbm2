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
    let layout = pane_scroll_layout(body, sel_row_w, total, body.height as usize);
    let content = layout.content_area;

    let viewport = content.height.max(1) as usize;
    let max_scroll = total.saturating_sub(viewport);

    // Discover-style anchor — skipped when scroll_locked (manual v_scrollbar drag).
    let scroll_locked = state.scroll_locked;
    let mut start = state.scroll.min(total.saturating_sub(1));
    if !scroll_locked {
        if state.cursor < start {
            start = state.cursor;
        } else if state.cursor >= start + viewport {
            start = state.cursor + 1 - viewport;
        }
    }

    Some(ObjectsViewport {
        body,
        layout,
        content,
        viewport,
        start,
        total,
        max_scroll,
    })
}

/// Result of [`v_scrollbar_hit`]: everything the drag handler needs.
pub struct ObjectsVScrollInfo {
    pub track_y: u16,
    pub max_scroll: usize,
    /// Track PIXEL height — drag formula needs this (NOT data-row count).
    pub viewport_height: usize,
}

/// Hit-test the objects pane's vertical scrollbar.
pub fn v_scrollbar_hit(
    area: Rect,
    state: &ObjectsState,
    x: u16,
    y: u16,
) -> Option<ObjectsVScrollInfo> {
    let ov = compute_objects_viewport(area, state)?;
    let v_bar = ov.layout.v_scrollbar?;
    if ov.max_scroll == 0 {
        return None;
    }
    if !crate::common::view::pane_scrollbar::point_in_bar(v_bar, x, y) {
        return None;
    }
    Some(ObjectsVScrollInfo {
        track_y: v_bar.y,
        max_scroll: ov.max_scroll,
        viewport_height: usize::from(v_bar.height.max(1)),
    })
}

/// Result of [`h_scrollbar_hit`]: everything the drag handler needs for the
/// horizontal scrollbar.
pub struct ObjectsHScrollInfo {
    pub track_x: u16,
    pub max_scroll: usize,
    /// Track PIXEL width — drag formula needs this.
    pub viewport_width: usize,
}

/// Hit-test the objects pane's horizontal scrollbar. Returns the drag
/// geometry if the click lands on the bar and the selected row actually
/// overflows the viewport.
pub fn h_scrollbar_hit(
    area: Rect,
    state: &ObjectsState,
    x: u16,
    y: u16,
) -> Option<ObjectsHScrollInfo> {
    let ov = compute_objects_viewport(area, state)?;
    let h_bar = ov.layout.h_scrollbar?;
    let sel_row_w = state.selected_row_width();
    let viewport_w = ov.content.width as usize;
    let max_scroll = sel_row_w.saturating_sub(viewport_w as u16) as usize;
    if max_scroll == 0 {
        return None;
    }
    if !crate::common::view::pane_scrollbar::point_in_bar(h_bar, x, y) {
        return None;
    }
    Some(ObjectsHScrollInfo {
        track_x: h_bar.x,
        max_scroll,
        viewport_width: usize::from(h_bar.width.max(1)),
    })
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
    let marker_col = ov
        .content
        .x
        .saturating_add(depth.saturating_mul(2))
        .saturating_sub(state.h_scroll);
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
        .border_style(p.active_border(region_focused));
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

    let mut lines = Vec::new();
    for idx in start..(start + viewport).min(ov.total) {
        let row = &state.rows[idx];
        let focused = idx == state.cursor;
        let style = if focused {
            Style::default()
                .fg(if row.active { p.selection_focus_text } else { p.selection_text })
                .bg(p.selection_bg)
                .add_modifier(Modifier::BOLD)
        } else if row.active {
            Style::default().fg(p.active_fg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg)
        };
        let marker = if row.expandable {
            if row.expanded { "▾" } else { "▸" }
        } else {
            "·"
        };
        let indent = "  ".repeat(row.depth);
        lines.push(Line::from(vec![
            Span::styled(format!("{indent}{marker} {}", row.label), style),
        ]));
    }
    if lines.is_empty() {
        let msg = if state.bound_connection.is_empty() {
            "Open a connection to browse objects"
        } else {
            "(no objects — press Enter on a connection to load the catalog)"
        };
        lines.push(Line::from(Span::styled(msg, Style::default().fg(p.muted))));
    }

    // Horizontal scrollbar — h_scroll pans content horizontally. The bar is
    // shown only when the selected row overflows the viewport (matching
    // history list), but Paragraph::scroll still uses full max_row_width so
    // ALL rows can scroll horizontally once the bar is visible.
    let sel_row_w = state.selected_row_width();
    let max_row_w = state.max_row_width();
    let viewport_w = layout.content_area.width as usize;
    let max_h_scroll = sel_row_w.saturating_sub(viewport_w as u16) as usize;
    let effective_h = state
        .h_scroll
        .min(max_row_w.saturating_sub(viewport_w as u16));
    let paragraph = Paragraph::new(lines).scroll((0, effective_h));
    frame.render_widget(paragraph, content);

    if let Some(bar) = layout.h_scrollbar {
        draw_horizontal_pane_scrollbar(
            frame,
            bar,
            state.h_scroll as usize,
            viewport_w,
            max_h_scroll,
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
