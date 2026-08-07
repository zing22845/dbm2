//! Discover feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::common::view::modal::render_popup;
use crate::common::view::theme::Theme;

use super::state::DiscoverState;
use super::engine::view as engine_view;
use super::results::view as results_view;
use super::targets::view as targets_view;

/// Render the discover feature: engine selector on top, with the targets editor
/// and results list stacked vertically below (mirrors the original dbm layout,
/// where targets sits above results and they are separated by a splitter row).
/// Each pane's border highlights when it owns focus, and the discover zone gets
/// a footer (pane nav + scan/close hints) across the bottom.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &DiscoverState,
    focus: crate::app_shell::nav::DiscoverPane,
) {
    use crate::common::view::hints::{discover_zone_footer_text, draw_pane_footer};
    let p = theme.palette();

    // The discover parent pane wraps the three child panes (engine, targets,
    // results) inside a single bordered block, matching the original dbm.
    let outer = ratatui::widgets::Block::default()
        .title(" Discover ")
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(p.border_active));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let footer_text = discover_zone_footer_text(&discover_status(state));
    let footer_h = if footer_text.is_empty() {
        0
    } else {
        // A separator row above the hint line(s).
        1 + footer_text.lines().count().clamp(1, 2) as u16
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),            // engine selector + its footer
            Constraint::Min(0),               // targets + results
            Constraint::Length(footer_h),     // discover zone footer
        ])
        .split(inner);

    engine_view::render(frame, theme, chunks[0], &state.engine, focus);

    // Stacked vertically, like the original: targets (top), a 1-row splitter,
    // then results (bottom). A vertical split means both panes are the same
    // width; the splitter keeps the horizontal divider visible.
    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50), // targets editor
            Constraint::Length(1),      // splitter
            Constraint::Percentage(50), // results list
        ])
        .split(chunks[1]);

    targets_view::render(frame, theme, body[0], &state.targets, focus);
    crate::common::view::splitter::draw(
        frame,
        body[1],
        crate::common::view::splitter::SplitOrientation::Horizontal,
        false,
        false,
    );
    results_view::render(frame, theme, body[2], &state.results, focus);

    // Discover zone footer: a separator row above the hint line(s), so the
    // hints are drawn on their own row instead of overlapping the dashes.
    if footer_h > 0 {
        let footer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(chunks[2]);
        let sep = Line::from(Span::styled(
            "-".repeat(footer[0].width as usize),
            ratatui::style::Style::default().fg(p.border),
        ));
        frame.render_widget(ratatui::widgets::Paragraph::new(sep), footer[0]);
        draw_pane_footer(frame, theme, footer[1], &footer_text);
    }

    // The close-confirmation dialog: asking to close the discover modal shows a
    // centered popup. Enter confirms (Close), Esc cancels (CancelClose); the
    // keys are routed in the discover input layer.
    if state.close_confirm {
        render_close_confirm(frame, theme, area);
    }
}

/// A one-line status for the discover zone footer (scanning / last error /
/// register result).
fn discover_status(state: &DiscoverState) -> String {
    if state.scanning {
        "scanning…".to_string()
    } else if state.last_error.is_some() {
        "scan failed".to_string()
    } else if let Some(msg) = &state.register_message {
        msg.clone()
    } else {
        String::new()
    }
}

/// Render the "close discovery?" confirmation popup over the whole discover
/// area. Pure `state -> view`: it only draws, never mutates state.
fn render_close_confirm(frame: &mut Frame, theme: &Theme, area: Rect) {
    let p = theme.palette();
    render_popup(frame, area, 45, 30, |frame, popup| {
        let block = ratatui::widgets::Block::default()
            .title(" Close discovery? ")
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(p.border_active))
            .style(ratatui::style::Style::default().bg(p.surface));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let body = vec![
            Line::from(Span::raw("This will close the discovery modal.")),
            Line::from(Span::raw("Any unsaved scan is discarded.")),
            Line::from(""),
            Line::from(Span::styled(
                "Confirm: ENTER    Cancel: ESC",
                ratatui::style::Style::default().fg(p.muted),
            )),
        ];
        frame.render_widget(ratatui::widgets::Paragraph::new(body), inner);
    });
}
