//! `sql_tab` feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;
use ratatui::widgets::{Block, Paragraph};

use super::state::SqlTabState;
use super::editor::view as editor_view;
use super::history::view as history_view;
use super::results::view as results_view;

/// Render the `sql_tab` feature: a tab bar plus the active tab's child panes.
pub fn render(frame: &mut Frame, area: Rect, state: &SqlTabState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tab bar
            Constraint::Min(0),    // child panes
        ])
        .split(area);

    // Tab bar: show the open-tab count and the active index.
    let tab_bar = Paragraph::new(format!(
        "Tabs: {} (active = {})",
        state.tabs.len(),
        state.active_tab
    ));
    frame.render_widget(tab_bar, chunks[0]);

    let Some(tab) = state.tabs.get(state.active_tab) else {
        // No tab is open: render an empty placeholder in the body.
        frame.render_widget(
            Block::default().title("No open SQL tab"),
            chunks[1],
        );
        return;
    };

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // editor
            Constraint::Percentage(50), // results over history
        ])
        .split(chunks[1]);

    editor_view::render(frame, body[0], &tab.editor);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(70), // results
            Constraint::Percentage(30), // history
        ])
        .split(body[1]);

    results_view::render(frame, right[0], &tab.results);
    history_view::render(frame, right[1], &tab.history);
}
