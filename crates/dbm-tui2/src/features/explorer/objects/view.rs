//! Explorer objects (object tree) feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::pane_scrollbar::{draw_horizontal_pane_scrollbar, pane_scroll_layout};
use crate::common::view::theme::Theme;

use super::state::ObjectsState;

/// Render the object tree with indentation and expansion markers.
/// `region_focused` controls the border color so the shell focus is visible.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ObjectsState,
    region_focused: bool,
) {
    let p = theme.palette();

    let mut lines = Vec::new();
    let inner_h = area.height.saturating_sub(2) as usize;
    for (vis, idx) in (state.scroll..state.rows.len()).enumerate() {
        if vis >= inner_h {
            break;
        }
        let row = &state.rows[idx];
        let focused = idx == state.cursor;
        let style = if focused {
            Style::default()
                .fg(p.selection)
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
        lines.push(Line::from(vec![
            Span::styled(format!("{indent}{marker} {}", row.label), style),
        ]));
    }
    if lines.is_empty() {
        let msg = if state.bound_connection.is_empty() {
            "(select a connection to browse objects)"
        } else {
            "(no objects — press Enter on a connection to load the catalog)"
        };
        lines.push(Line::from(Span::styled(msg, Style::default().fg(p.muted))));
    }

    let border_color = if region_focused { p.border_active } else { p.border };
    let block = Block::default()
        .title(" objects ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(area);
    // The widest rendered row drives the horizontal scrollbar: it only appears
    // when content is wider than the text viewport, and its thumb position
    // reflects `h_scroll` so the user can tell at a glance whether the content
    // is scrolled to its end (matching the original dbm).
    let max_row_w = state.max_row_width();
    let layout = pane_scroll_layout(
        inner,
        max_row_w,
        lines.len(),
        inner.height as usize,
    );
    let viewport_w = layout.content_area.width as usize;
    let effective_h = state
        .h_scroll
        .min(max_row_w.saturating_sub(viewport_w as u16));
    // Border on `area`; content (clipped + horizontally panned) on `content_area`.
    frame.render_widget(&block, area);
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
}
