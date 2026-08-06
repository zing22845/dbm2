//! Discover feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
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
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &DiscoverState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // engine selector
            Constraint::Min(0),    // targets + results
        ])
        .split(area);

    engine_view::render(frame, theme, chunks[0], &state.engine);

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

    targets_view::render(frame, theme, body[0], &state.targets);
    crate::common::view::splitter::draw(
        frame,
        body[1],
        crate::common::view::splitter::SplitOrientation::Horizontal,
        false,
        false,
    );
    results_view::render(frame, theme, body[2], &state.results);

    // The close-confirmation dialog: asking to close the discover modal shows a
    // centered popup. Enter confirms (Close), Esc cancels (CancelClose); the
    // keys are routed in the discover input layer.
    if state.close_confirm {
        render_close_confirm(frame, theme, area);
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
