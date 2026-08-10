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
    use crate::common::utils::text_width::wrapped_line_count;
    use crate::common::view::hints::{discover_results_footer_text, draw_pane_footer};
    let p = theme.palette();
    let focused = focus == crate::app_shell::nav::DiscoverPane::Results;

    let footer_text = discover_results_footer_text();

    let block = Block::default()
        .title(" results ")
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // The footer is a keys line plus a mark legend line; size it to its wrapped
    // height so the legend survives a narrow pane.
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
            // Row mark matches the original dbm: an already-registered instance
            // is `×`, a selected (unregistered) one is `✓`, anything else is
            // blank — not registered rows carry no mark.
            let checked = result_selection_glyph(item.already_registered, state.selected.contains(&idx));
            let style = if row_focused {
                Style::default()
                    .fg(p.fg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(p.fg)
            };
            // A focused row gets the unified row-selection background across the
            // whole line (mirroring the original dbm's connections selection).
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
    }
    frame.render_widget(Paragraph::new(lines), body);

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
        // Unregistered + unselected -> no mark.
        assert_eq!(result_selection_glyph(false, false), " ");
        // Unregistered + selected -> ✓.
        assert_eq!(result_selection_glyph(false, true), "✓");
        // Registered rows are always × regardless of selection.
        assert_eq!(result_selection_glyph(true, false), "×");
        assert_eq!(result_selection_glyph(true, true), "×");
    }
}
