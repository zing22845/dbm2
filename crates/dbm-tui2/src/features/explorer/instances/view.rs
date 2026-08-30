//! Explorer instances (connection tree) feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::hints::{draw_pane_footer, footer_height, instances_pane_footer_text};
use crate::common::view::pane_scrollbar::{
    PaneScrollLayout, draw_horizontal_pane_scrollbar, draw_vertical_pane_scrollbar,
    pane_scroll_layout,
};
use crate::common::view::theme::Theme;

use super::state::InstancesState;

/// Result of [`compute_instances_viewport`]: shared by render, row_at, toggle_at,
/// and v_scrollbar_hit so they agree on body geometry, viewport start, and
/// scrollbar placement.
pub struct InstancesViewport {
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
fn compute_instances_body(area: Rect, state: &InstancesState) -> Option<Rect> {
    let instance_row = state
        .cursor_selection()
        .map(|(_, conn)| conn.is_none())
        .unwrap_or(false);
    let hint = instances_pane_footer_text(instance_row);
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

/// Compute the instances tree viewport. One source of truth for render, row_at,
/// toggle_at, and v_scrollbar_hit. Applies discover-style cursor anchoring
/// (skipped when `scroll_locked`).
pub fn compute_instances_viewport(
    area: Rect,
    state: &InstancesState,
) -> Option<InstancesViewport> {
    let total = state.visible_count();
    if total == 0 {
        return None;
    }
    let body = compute_instances_body(area, state)?;
    // Horizontal scrollbar shown only when the CURRENTLY SELECTED row overflows
    // (matching history list), not the widest row in the tree.
    let sel_row_w = state.selected_row_width();
    // First pass: reserve bars assuming the selected row might overflow.
    let first_pass = pane_scroll_layout(body, sel_row_w, total, body.height as usize);
    // Second pass (effective_layout): if the first pass's content_area is
    // already wide enough for the selected row, drop h_scrollbar.
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

    // Discover-style anchor via shared helper — cursor only pushes the viewport
    // when it would fall outside; skipped when scroll_locked (manual drag).
    let start = crate::common::view::pane_scrollbar::discover_anchor(
        state.scroll.get(),
        max_scroll,
        state.cursor,
        viewport,
        state.scroll_locked,
    );
    // Write back so the next frame starts from the correct position, not stale 0.
    state.scroll.set(start);

    // Write the viewport-aware max back so update's ScrollHorizontal / SetHScroll
    // can clamp to the real upper bound and stay in sync.
    state.cached_h_max_scroll.set(max_h_scroll);

    Some(InstancesViewport {
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

/// Hit-test the instances pane's vertical scrollbar — delegates to the shared
/// `pane_scrollbar::v_scrollbar_hit` helper after computing the viewport.
pub fn v_scrollbar_hit(
    area: Rect,
    state: &InstancesState,
    x: u16,
    y: u16,
) -> Option<ScrollbarHitInfo> {
    let iv = compute_instances_viewport(area, state)?;
    crate::common::view::pane_scrollbar::v_scrollbar_hit(&iv.layout, iv.max_scroll, x, y)
}

/// Hit-test the instances pane's horizontal scrollbar.
pub fn h_scrollbar_hit(
    area: Rect,
    state: &InstancesState,
    x: u16,
    y: u16,
) -> Option<ScrollbarHitInfo> {
    let iv = compute_instances_viewport(area, state)?;
    crate::common::view::pane_scrollbar::h_scrollbar_hit(&iv.layout, iv.max_h_scroll, x, y)
}

/// Hit-test a click inside the instances tree area to a visible row (absolute,
/// including scroll offset), mirroring `render`'s body geometry. Returns `None`
/// when the click is on the border, title, footer, or beyond the row count.
///
/// IMPORTANT: previous versions intentionally ignored `state.scroll` because
/// session restore could set a stale nonzero value. Now that render uses scroll
/// properly for vertical slicing, row_at MUST add it back — otherwise a click
/// below the viewport would hit the wrong row.
pub fn row_at(area: Rect, state: &InstancesState, y: u16) -> Option<usize> {
    let iv = compute_instances_viewport(area, state)?;
    let content = iv.content;
    if y < content.y || y >= content.y + content.height {
        return None;
    }
    let row_in_content = (y - content.y) as usize;
    let data_row = row_in_content.min(iv.viewport.saturating_sub(1));
    let row_idx = iv.start + data_row;
    if row_idx < iv.total {
        Some(row_idx)
    } else {
        None
    }
}

/// Like [`row_at`], but also require the click x to land on the row's
/// expand/collapse marker (`▸`/`▾`). Returns the visible row when the marker
/// was clicked, so the caller can toggle expansion. Marker columns account for
/// the connection indentation and the horizontal scroll.
pub fn toggle_at(area: Rect, state: &InstancesState, x: u16, y: u16) -> Option<usize> {
    let row = row_at(area, state, y)?;
    let (is_connection, _) = state.visible_row_is_connection(row);
    if is_connection {
        return None;
    }
    let iv = compute_instances_viewport(area, state)?;
    // Instance rows render as " {marker}": marker is the 2nd char after the
    // leading space, at content.x + 1 (after leading space).
    let marker_col = iv.content
        .x
        .saturating_add(1)
        .saturating_sub(state.h_scroll);
    // Marker is a single wide char; a couple of columns of tolerance.
    (x >= marker_col && x < marker_col.saturating_add(2)).then_some(row)
}

/// Render the instances connection tree. `region_focused` controls the border
/// color so the shell focus is visible (active border vs. muted border). A pane
/// footer hint occupies the bottom rows, wrapping to the pane width.
///
/// Now uses pane_scroll_layout (v_scrollbar reservation + discover-style
/// cursor anchor) — same pattern as history/results and discover targets.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &InstancesState,
    region_focused: bool,
) {
    let p = theme.palette();

    let instance_row = state
        .cursor_selection()
        .map(|(_, conn)| conn.is_none())
        .unwrap_or(false);
    let hint = instances_pane_footer_text(instance_row);
    let footer_h = footer_height(&hint, area.width.saturating_sub(2)).min(area.height.saturating_sub(2));
    let block = Block::default()
        .title(" [I] Instances ")
        .borders(Borders::ALL)
        .border_style(p.active_border(region_focused));
    frame.render_widget(&block, area);
    let inner = block.inner(area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // ---- SHARED VIEWPORT CALCULATION ----
    let iv = match compute_instances_viewport(area, state) {
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

    let content = iv.content;
    let layout = &iv.layout;
    let start = iv.start;
    let viewport = iv.viewport;
    let body = iv.body;
    let viewport_w = layout.content_area.width as usize;
    let max_h = iv.max_h_scroll;
    let effective_h = state.h_scroll.min(max_h as u16);

    // Build visible rows lazily: iterate nodes + connections, track flat row
    // index, skip rows before `start`, emit rows while within viewport.
    let mut lines = Vec::new();
    let mut flat_row = 0usize;
    let mut rows_emitted = 0usize;
    'outer: for (inst_idx, node) in state.nodes.iter().enumerate() {
        // Instance row.
        if flat_row >= start && rows_emitted < viewport {
            let instance_name = node
                .instance
                .as_ref()
                .map(|i| i.name.clone())
                .unwrap_or_default();
            let focused = flat_row == state.cursor;
            let active = state.is_active_instance(inst_idx);
            let style = if focused {
                Style::default()
                    .fg(if active { p.selection_focus_text } else { p.selection_text })
                    .bg(p.selection_bg)
                    .add_modifier(Modifier::BOLD)
            } else if active {
                Style::default().fg(p.active_fg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(p.fg)
            };
            let marker = if node.expanded { "▾" } else { "▸" };
            let full_text = format!(" {marker} {instance_name}");
            let row_h_scroll: usize = if focused { effective_h as usize } else { 0 };
            let display_text = crate::common::utils::text_width::truncate_plain_from(
                &full_text, row_h_scroll, viewport_w,
            );
            lines.push(Line::from(vec![Span::styled(display_text, style)]));
            rows_emitted += 1;
        }
        flat_row += 1;
        if rows_emitted >= viewport {
            break;
        }
        if node.expanded {
            for (ci, conn) in node.connections.iter().enumerate() {
                if flat_row >= start && rows_emitted < viewport {
                    let conn_focused = flat_row == state.cursor;
                    let conn_active = state.is_active_connection(inst_idx, ci);
                    let cstyle = if conn_focused {
                        Style::default()
                            .fg(if conn_active { p.selection_focus_text } else { p.selection_text })
                            .bg(p.selection_bg)
                            .add_modifier(Modifier::BOLD)
                    } else if conn_active {
                        Style::default().fg(p.active_fg).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(p.fg)
                    };
                    let full_text = format!("    └ {}/{}", conn.name, conn.database);
                    let row_h_scroll: usize = if conn_focused { effective_h as usize } else { 0 };
                    let display_text = crate::common::utils::text_width::truncate_plain_from(
                        &full_text, row_h_scroll, viewport_w,
                    );
                    lines.push(Line::from(vec![Span::styled(display_text, cstyle)]));
                    rows_emitted += 1;
                }
                flat_row += 1;
                if rows_emitted >= viewport {
                    break 'outer;
                }
            }
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no instances — run discover)",
            Style::default().fg(p.muted),
        )));
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
            iv.max_scroll,
            p,
            false,
        );
    }

    // Pane footer.
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
    use crate::features::explorer::instances::state::InstanceNode;

    #[test]
    fn row_at_maps_click_to_visible_row() {
        let mut s = InstancesState::default();
        s.set_instances(vec![dbm_store::ManagedInstance {
            id: "a".into(),
            fingerprint: "a".into(),
            name: "a".into(),
            engine: dbm_core::Engine::Postgres,
            host: "h".into(),
            port: 1,
            socket_path: None,
            data_dir: None,
            env_label: None,
            registered_at: "now".into(),
            version_full: None,
            version_short: None,
            version_checked_at: None,
            lifecycle_status: None,
            lifecycle_checked_at: None,
            lifecycle_detail: None,
        }]);
        let area = Rect::new(0, 5, 40, 20);
        assert_eq!(row_at(area, &s, 6), Some(0));
        assert_eq!(row_at(area, &s, 5), None);
    }

    #[test]
    fn row_at_adds_back_scroll_offset() {
        // With v_scroll now enabled, row_at must include scroll so clicks below
        // the viewport hit the correct row.
        let mut s = InstancesState::default();
        s.set_instances(vec![dbm_store::ManagedInstance {
            id: "a".into(),
            fingerprint: "a".into(),
            name: "a".into(),
            engine: dbm_core::Engine::Postgres,
            host: "h".into(),
            port: 1,
            socket_path: None,
            data_dir: None,
            env_label: None,
            registered_at: "now".into(),
            version_full: None,
            version_short: None,
            version_checked_at: None,
            lifecycle_status: None,
            lifecycle_checked_at: None,
            lifecycle_detail: None,
        }]);
        s.nodes.push(InstanceNode {
            instance: Some(dbm_store::ManagedInstance {
                id: "b".into(),
                fingerprint: "b".into(),
                name: "b".into(),
                engine: dbm_core::Engine::Postgres,
                host: "h2".into(),
                port: 2,
                socket_path: None,
                data_dir: None,
                env_label: None,
                registered_at: "now".into(),
                version_full: None,
                version_short: None,
                version_checked_at: None,
                lifecycle_status: None,
                lifecycle_checked_at: None,
                lifecycle_detail: None,
            }),
            ..Default::default()
        });
        s.scroll.set(1); // scrolled to row 1 (instance b)
        s.scroll_locked = true; // manual scroll — prevent cursor anchor from resetting it
        let area = Rect::new(0, 5, 40, 5); // very small: only 1 body row after borders+footer
        // Row at content top (body_top = 6) should be row 1 (instance b).
        assert_eq!(row_at(area, &s, 6), Some(1), "scrolled viewport must add start offset");
    }
}
