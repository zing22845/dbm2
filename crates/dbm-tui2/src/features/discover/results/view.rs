//! Discovery results feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::ResultsState;

/// Render the results list of discovered instances. The border highlights only
/// when the results pane owns focus, and a footer line shows the results keys.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ResultsState,
    focus: crate::app_shell::nav::DiscoverPane,
) {
    use crate::common::view::hints::{discover_results_footer_text, draw_pane_footer};
    let p = theme.palette();
    let focused = focus == crate::app_shell::nav::DiscoverPane::Results;

    let footer_text = discover_results_footer_text();
    let footer_h = if footer_text.is_empty() { 0 } else { 1 };

    let block = Block::default()
        .title(" results ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { p.border_active } else { p.border }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let body = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer_h),
    );
    let mut lines = Vec::new();
    if state.is_empty() {
        lines.push(Line::from(Span::styled(
            "(scan to discover instances)",
            Style::default().fg(p.muted),
        )));
    } else {
        // Cursor/scroll are positions within the filtered (visible) list, so
        // iterate the visible items indices and use the display position for
        // focus/selection markers. `selected` stores underlying `items` indices.
        let visible = state.visible_indices();
        let body_h = body.height as usize;
        for (vis, &idx) in visible.iter().enumerate().skip(state.scroll) {
            if vis - state.scroll >= body_h {
                break;
            }
            let item = &state.items[idx];
            let row_focused = vis == state.cursor;
            let checked = if state.selected.contains(&idx) { "✓" } else { " " };
            let style = if row_focused {
                Style::default()
                    .fg(p.selection)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(p.fg)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("[{checked}]"), Style::default().fg(p.accent)),
                Span::styled(format!("  {}:{}", item.host, item.port), style),
                Span::styled(
                    format!("  ({})", item.confidence.as_str()),
                    Style::default().fg(p.muted),
                ),
            ]));
        }
    }
    frame.render_widget(Paragraph::new(lines), body);

    if footer_h > 0 {
        let footer_area = Rect::new(inner.x, inner.y + body.height, inner.width, footer_h);
        draw_pane_footer(frame, theme, footer_area, &footer_text);
    }
}
