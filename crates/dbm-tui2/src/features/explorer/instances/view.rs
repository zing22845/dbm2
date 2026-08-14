//! Explorer instances (connection tree) feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::hints::{draw_pane_footer, footer_height, instances_pane_footer_text};
use crate::common::view::pane_scrollbar::{draw_horizontal_pane_scrollbar, pane_scroll_layout};
use crate::common::view::theme::Theme;

use super::state::InstancesState;

/// Hit-test a click inside the instances tree area to a visible row (absolute,
/// including scroll offset), mirroring `render`'s body geometry. Returns `None`
/// when the click is on the border, title, footer, or beyond the row count.
pub fn row_at(area: Rect, state: &InstancesState, y: u16) -> Option<usize> {
    let hint = instances_pane_footer_text(
        state.cursor_selection().map(|(_, c)| c.is_none()).unwrap_or(false),
    );
    let footer_h =
        footer_height(&hint, area.width.saturating_sub(2)).min(area.height.saturating_sub(2));
    let inner_h = area.height.saturating_sub(2); // borders
    let body_h = inner_h.saturating_sub(footer_h);
    let body_top = area.y.saturating_add(1); // top border
    if y >= body_top && y < body_top.saturating_add(body_h) {
        let row = (y - body_top) as usize + state.scroll;
        if row < state.visible_count() {
            return Some(row);
        }
    }
    None
}

/// Like [`row_at`], but also require the click x to land on the row's
/// expand/collapse marker (`▸`/`▾`). Returns the visible row when the marker
/// was clicked, so the caller can toggle expansion. Marker columns account for
/// the connection indentation and the horizontal scroll.
pub fn toggle_at(area: Rect, state: &InstancesState, x: u16, y: u16) -> Option<usize> {
    let row = row_at(area, state, y)?;
    // Only instance rows have an expand/collapse marker; connection rows do not.
    let (is_connection, _) = state.visible_row_is_connection(row);
    if is_connection {
        return None;
    }
    // Recompute the body origin (mirrors `render`): top border at area.y+1.
    let body_x = area.x.saturating_add(1);
    // Instance rows render as " {marker}": marker is the 2nd char of the body.
    let marker_col = body_x.saturating_add(1).saturating_sub(state.h_scroll);
    // Marker is a single wide char; a couple of columns of tolerance.
    (x >= marker_col && x < marker_col.saturating_add(2)).then_some(row)
}

/// Render the instances connection tree. `region_focused` controls the border
/// color so the shell focus is visible (active border vs. muted border). A pane
/// footer hint occupies the bottom rows, wrapping to the pane width.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &InstancesState,
    region_focused: bool,
) {
    let p = theme.palette();

    // The block is drawn over `area`; its inner area is split into a body (the
    // tree, with a horizontal scrollbar) and a footer hint area at the bottom,
    // both *inside* the pane's border — matching the original dbm. The footer
    // is sized to its wrapped height so a narrow terminal does not clip it.
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
    let mut row = 0usize;
    'outer: for (inst_idx, node) in state.nodes.iter().enumerate() {
        let instance_name = node
            .instance
            .as_ref()
            .map(|i| i.name.clone())
            .unwrap_or_default();
        let focused = row == state.cursor;
        let active = state.is_active_instance(inst_idx);
        // The active workspace is distinguished by a dedicated `active_fg` color
        // (shared by instance and connection rows). The cursor row only layers
        // the selection highlight on top; when the active row is also the cursor
        // row it keeps its selection background and the regular active color.
        let style = if focused {
            Style::default()
                .fg(if active { p.active_fg } else { p.fg })
                .bg(p.selection_bg)
                .add_modifier(Modifier::BOLD)
        } else if active {
            Style::default().fg(p.active_fg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg)
        };
        let marker = if node.expanded { "▾" } else { "▸" };
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} {instance_name}"), style),
        ]));
        row += 1;
        if row > inner_h {
            break;
        }
        if node.expanded {
            for (ci, conn) in node.connections.iter().enumerate() {
                let conn_focused = row == state.cursor;
                let conn_active = state.is_active_connection(inst_idx, ci);
                // Same unified active color as the instance row; the cursor row
                // layers the selection highlight underneath. A non-active
                // connection keeps its normal foreground (no purple tint) even
                // when the cursor is on it — only the active connection is
                // highlighted.
                let cstyle = if conn_focused {
                    Style::default()
                        .fg(if conn_active { p.active_fg } else { p.fg })
                        .bg(p.selection_bg)
                        .add_modifier(Modifier::BOLD)
                } else if conn_active {
                    Style::default().fg(p.active_fg).add_modifier(Modifier::BOLD)
                } else {
                    // Inactive connections share the instance row's foreground
                    // color (the active one is highlighted separately).
                    Style::default().fg(p.fg)
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("    └ {}/{}", conn.name, conn.database), cstyle),
                ]));
                row += 1;
                if row > inner_h {
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

    // Pane footer (inside the border): hints differ for an instance row vs a
    // connection row.
    draw_pane_footer(frame, theme, footer_area, &hint);
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // area at y=5: body starts at y=6 (top border). Clicking row 6 -> row 0.
        let area = Rect::new(0, 5, 40, 20);
        assert_eq!(row_at(area, &s, 6), Some(0));
        // Clicking on the border/title (y=5) -> none.
        assert_eq!(row_at(area, &s, 5), None);
    }

    #[test]
    fn toggle_at_hits_the_marker_column_only() {
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
        // Body x starts at area.x+1 = 1; instance marker is the 2nd char, at
        // body.x+1 = 2.
        assert_eq!(toggle_at(area, &s, 2, 6), Some(0), "marker column hits");
        // Clicking the leading space (x=1) or the label (x=5) is not the marker.
        assert_eq!(toggle_at(area, &s, 1, 6), None);
        assert_eq!(toggle_at(area, &s, 5, 6), None);
        // Clicking the border/title row is not a marker.
        assert_eq!(toggle_at(area, &s, 2, 5), None);
    }

    #[test]
    fn render_places_first_instance_row_at_row_at_body_top() {
        // Render an instances pane and confirm the first instance row is drawn
        // at the same y that `row_at` treats as row 0 (body_top = area.y+1), so
        // a click lands on the visually correct row.
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let theme = crate::common::view::theme::dracula();
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
        let mut terminal = Terminal::new(TestBackend::new(40, 25)).unwrap();
        terminal
            .draw(|frame| {
                let theme = theme.clone();
                render(frame, &theme, area, &s, true);
            })
            .unwrap();
        // Find the y of the first non-border row containing the instance name.
        let buf = terminal.backend().buffer();
        let mut found_y = None;
        for y in 0..25 {
            let mut line = String::new();
            for x in 0..40 {
                line.push_str(buf[(x, y)].symbol());
            }
            if line.contains("a") && !line.contains("Instances") {
                found_y = Some(y as u16);
                break;
            }
        }
        // row_at treats body_top = area.y+1 = 6 as row 0, so the first instance
        // row must render at y=6 (not 5).
        assert_eq!(found_y, Some(6), "first instance row is at body_top (area.y+1)");
        assert_eq!(row_at(area, &s, 6), Some(0));
        // A click one row higher hits the border/title, not a row.
        assert_eq!(row_at(area, &s, 5), None);
    }
}
