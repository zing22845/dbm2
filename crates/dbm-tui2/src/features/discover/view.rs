//! Discover feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::DiscoverState;
use super::engine::view as engine_view;
use super::results::view as results_view;
use super::targets::view as targets_view;

/// Render the discover feature: engine selector on top, with the targets editor
/// and results list stacked vertically below (mirrors the original dbm layout,
/// where targets sits above results and they are separated by a splitter row).
/// Each pane's border highlights when it owns focus, and the discover dialog
/// gets a footer (pane nav + scan/close hints) across the bottom.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &DiscoverState,
    focus: crate::app_shell::nav::DiscoverPane,
) {
    use crate::common::view::hints::{discover_footer_text, draw_pane_footer};
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

    let footer_text = discover_footer_text(&discover_status(state));
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
            Constraint::Length(footer_h),     // discover dialog footer
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

    // Discover dialog footer: a separator row above the hint line(s), so the
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

/// A one-line status for the discover dialog footer (scanning with live
/// progress / cancelling / cancelled / last error / register result). Mirrors
/// the original dbm's `Scanning… hosts d/t · c: cancel` status line.
fn discover_status(state: &DiscoverState) -> String {
    if state.scanning {
        if state.cancelling {
            "cancelling…".to_string()
        } else if let Some((done, total)) = state.scan_progress {
            format!("Scanning… hosts {done}/{total} · c: cancel")
        } else {
            "Scanning…".to_string()
        }
    } else if state.scan_cancelled {
        "cancelled".to_string()
    } else if state.last_error.is_some() {
        "scan failed".to_string()
    } else if let Some(msg) = &state.register_message {
        msg.clone()
    } else {
        String::new()
    }
}

/// Render the "close discovery?" confirmation popup over the whole discover
/// area, matching the original dbm's close-confirm dialog: a centered popup
/// with a message row and a `Yes`/`No` button row. Pure `state -> view`: it
/// only draws, never mutates state.
fn render_close_confirm(frame: &mut Frame, theme: &Theme, area: Rect) {
    let p = theme.palette();

    let popup_w = area.width.clamp(28, 44);
    let popup_h = 5u16;
    let popup = Rect {
        x: area.x.saturating_add(area.width.saturating_sub(popup_w) / 2),
        y: area.y.saturating_add(area.height.saturating_sub(popup_h) / 2),
        width: popup_w,
        height: popup_h.min(area.height),
    };
    if popup.width == 0 || popup.height == 0 {
        return;
    }

    // Only the popup's own rectangle gets cleared/filled; the rest of the
    // discover pane stays visible around the confirmation dialog.
    crate::common::view::overlay_clear::clear_overlay(frame, popup);
    frame.render_widget(
        ratatui::widgets::Block::default().style(ratatui::style::Style::default().bg(p.surface)),
        popup,
    );

    let block = ratatui::widgets::Block::default()
        .title(" Close Discover ")
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(p.border))
        .style(ratatui::style::Style::default().bg(p.surface));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height < 2 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(
        ratatui::widgets::Paragraph::new("Close Discover and return to the tree?")
            .style(ratatui::style::Style::default().bg(p.surface)),
        chunks[0],
    );

    // Button row: a highlighted " Yes " and a plain " No ", centered.
    let btn_row = chunks[1];
    let yes_label = " Yes ";
    let no_label = " No ";
    let gap = 3u16;
    let yes_w = yes_label.chars().count() as u16;
    let no_w = no_label.chars().count() as u16;
    let total = yes_w.saturating_add(gap).saturating_add(no_w);
    let start_x = btn_row
        .x
        .saturating_add(btn_row.width.saturating_sub(total) / 2);
    let yes_rect = Rect {
        x: start_x,
        y: btn_row.y,
        width: yes_w.min(btn_row.width),
        height: 1,
    };
    let no_rect = Rect {
        x: start_x.saturating_add(yes_w).saturating_add(gap),
        y: btn_row.y,
        width: no_w,
        height: 1,
    };
    let yes_style = ratatui::style::Style::default()
        .fg(p.selection)
        .add_modifier(ratatui::style::Modifier::BOLD);
    let no_style = ratatui::style::Style::default().bg(p.surface);
    frame.render_widget(
        ratatui::widgets::Paragraph::new(yes_label).style(yes_style),
        yes_rect,
    );
    if no_rect.x + no_rect.width <= btn_row.right() {
        frame.render_widget(
            ratatui::widgets::Paragraph::new(no_label).style(no_style),
            no_rect,
        );
    }
}
