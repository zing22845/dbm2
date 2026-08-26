//! Explorer objects (object tree) feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::hints::{draw_pane_footer, footer_height, objects_pane_footer_text};
use crate::common::view::pane_scrollbar::{draw_horizontal_pane_scrollbar, pane_scroll_layout};
use crate::common::view::theme::Theme;

use super::state::ObjectsState;

/// Hit-test a click inside the objects tree area to a visible row (absolute,
/// including scroll offset), mirroring `render`'s body geometry. Returns `None`
/// when the click is on the border, title, footer, or beyond the row count.
pub fn row_at(area: Rect, state: &ObjectsState, y: u16) -> Option<usize> {
    let footer_h =
        footer_height(&objects_pane_footer_text(), area.width.saturating_sub(2)).min(area.height.saturating_sub(2));
    let inner_h = area.height.saturating_sub(2); // borders
    let body_h = inner_h.saturating_sub(footer_h);
    let body_top = area.y.saturating_add(1); // top border
    if y >= body_top && y < body_top.saturating_add(body_h) {
        let row = (y - body_top) as usize + state.scroll;
        if row < state.rows.len() {
            return Some(row);
        }
    }
    None
}

/// Like [`row_at`], but also require the click x to land on the row's
/// expand/collapse marker (`▸`/`▾`). Returns the visible row when the marker
/// was clicked, so the caller can toggle expansion. The marker column accounts
/// for the row's depth indentation and the horizontal scroll.
pub fn toggle_at(area: Rect, state: &ObjectsState, x: u16, y: u16) -> Option<usize> {
    let row = row_at(area, state, y)?;
    let depth = state.rows.get(row).map(|r| r.depth).unwrap_or(0) as u16;
    // Rows render as "{indent}{marker} label" with indent = 2 cols per depth
    // and NO leading space, so the marker sits at body_x + depth*2 (matching
    // `render`'s `format!("{indent}{marker} {label}")`). No +1 leading-space
    // offset here, unlike the instances pane (which does emit a leading space).
    let body_x = area.x.saturating_add(1); // left border
    let marker_col = body_x
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

    // The block is drawn over `area`; its inner area is split into a body (the
    // tree, with a horizontal scrollbar) and a footer hint area at the bottom,
    // both *inside* the pane's border — matching the original dbm. The footer
    // is sized to its wrapped height so a narrow terminal does not clip it.
    let hint = objects_pane_footer_text();
    let footer_h = footer_height(&hint, area.width.saturating_sub(2)).min(area.height.saturating_sub(2));
    let block = Block::default()
        .title(" [O] Objects ")
        .borders(Borders::ALL)
        .border_style(p.active_border(region_focused));
    frame.render_widget(&block, area);
    let inner = block.inner(area);
    let (body, footer_area) = if inner.height > footer_h {
        let h = inner.height.saturating_sub(footer_h);
        (
            Rect {
                x: inner.x,
                y: inner.y,
                width: inner.width,
                height: h,
            },
            Rect {
                x: inner.x,
                y: inner.y.saturating_add(h),
                width: inner.width,
                height: footer_h,
            },
        )
    } else {
        (inner, Rect::default())
    };

    let mut lines = Vec::new();
    let inner_h = body.height as usize;
    for (vis, idx) in (state.scroll..state.rows.len()).enumerate() {
        if vis >= inner_h {
            break;
        }
        let row = &state.rows[idx];
        let focused = idx == state.cursor;
        // The active schema row is highlighted (dedicated active color + bold);
        // the cursor row keeps its selection highlight underneath.
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
            // No active connection (active workspace is an instance or nothing):
            // prompt to open a connection, matching the original dbm.
            "Open a connection to browse objects"
        } else {
            "(no objects — press Enter on a connection to load the catalog)"
        };
        lines.push(Line::from(Span::styled(msg, Style::default().fg(p.muted))));
    }

    // The widest rendered row drives the horizontal scrollbar: it only appears
    // when content is wider than the text viewport, and its thumb position
    // reflects `h_scroll` so the user can tell at a glance whether the content
    // is scrolled to its end (matching the original dbm).
    let max_row_w = state.max_row_width();
    let layout = pane_scroll_layout(
        body,
        max_row_w,
        lines.len(),
        body.height as usize,
    );
    let viewport_w = layout.content_area.width as usize;
    let effective_h = state
        .h_scroll
        .min(max_row_w.saturating_sub(viewport_w as u16));
    // Content (clipped + horizontally panned) on the body's content_area.
    let paragraph = Paragraph::new(lines).scroll((0, effective_h));
    frame.render_widget(paragraph, layout.content_area);
    if let Some(bar) = layout.h_scrollbar {
        let max_scroll = max_row_w.saturating_sub(viewport_w as u16) as usize;
        draw_horizontal_pane_scrollbar(
            frame,
            bar,
            state.h_scroll as usize,
            viewport_w,
            max_scroll,
            p,
            false,
        );
    }

    // Pane footer (inside the border).
    draw_pane_footer(frame, theme, footer_area, &hint);
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
