//! Explorer instances (connection tree) feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::hints::{draw_pane_footer, instances_pane_footer_text};
use crate::common::view::pane_scrollbar::{draw_horizontal_pane_scrollbar, pane_scroll_layout};
use crate::common::view::theme::Theme;

use super::state::InstancesState;

/// Render the instances connection tree. `region_focused` controls the border
/// color so the shell focus is visible (active border vs. muted border). A pane
/// footer hint line occupies the bottom row.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &InstancesState,
    region_focused: bool,
) {
    let p = theme.palette();

    // The block is drawn over `area`; its inner area is split into a body (the
    // tree, with a horizontal scrollbar) and a footer hint line at the bottom,
    // both *inside* the pane's border — matching the original dbm.
    let block = Block::default()
        .title(" instances ")
        .borders(Borders::ALL)
        .border_style(p.active_border(region_focused));
    frame.render_widget(&block, area);
    let inner = block.inner(area);
    let (body, footer_area) = if inner.height > 1 {
        let h = inner.height.saturating_sub(1);
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
                height: 1,
            },
        )
    } else {
        (inner, Rect::default())
    };

    let mut lines = Vec::new();
    let inner_h = body.height as usize;
    let mut row = 0usize;
    'outer: for node in &state.nodes {
        let instance_name = node
            .instance
            .as_ref()
            .map(|i| i.name.clone())
            .unwrap_or_default();
        let focused = row == state.cursor;
        let style = if focused {
            Style::default()
                .fg(p.fg)
                .bg(p.selection_bg)
                .add_modifier(Modifier::BOLD)
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
            for conn in &node.connections {
                let conn_focused = row == state.cursor;
                let cstyle = if conn_focused {
                    Style::default()
                        .fg(p.accent)
                        .bg(p.selection_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(p.fg_dim)
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
    let instance_row = state
        .cursor_selection()
        .map(|(_, conn)| conn.is_none())
        .unwrap_or(false);
    draw_pane_footer(
        frame,
        theme,
        footer_area,
        &instances_pane_footer_text(instance_row),
    );
}
