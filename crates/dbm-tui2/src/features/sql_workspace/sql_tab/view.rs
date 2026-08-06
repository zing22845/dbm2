//! `sql_tab` feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;
use ratatui::widgets::Block;

use crate::common::view::theme::Theme;

use super::session::TabSession;
use super::state::SqlTabState;
use super::editor::view as editor_view;
use super::history::view as history_view;
use super::results::view as results_view;

/// Render the `sql_tab` feature: a tab bar plus the active tab's child panes.
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &SqlTabState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tab bar
            Constraint::Min(0),    // child panes
        ])
        .split(area);

    // Tab bar: themed tabs with session-derived titles + click rects.
    let sessions: Vec<TabSession> = state.tabs.iter().map(|t| t.session.clone()).collect();
    super::tab::render(frame, theme, chunks[0], &sessions, Some(state.active_tab));

    let Some(tab) = state.tabs.get(state.active_tab) else {
        // No tab is open: render an empty placeholder in the body.
        frame.render_widget(
            Block::default().title("No open SQL tab"),
            chunks[1],
        );
        return;
    };

    // Layout mirrors the original dbm `sql_tab_layout` (ui.rs §11): a vertical
    // split puts the editor+history row on top and results on the bottom; the
    // top row is a horizontal split with the SQL editor on the left and the
    // query history on the right.
    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(45), // top row: editor + history
            Constraint::Min(6),         // bottom: results
        ])
        .split(chunks[1]);

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(55), // editor
            Constraint::Percentage(45), // history
        ])
        .split(body[0]);

    editor_view::render(frame, theme, top[0], &tab.editor);

    let (instance, connection) = session_view_key(&tab.session);
    history_view::render(frame, theme, top[1], &tab.history, &instance, &connection);

    results_view::render(frame, theme, body[1], &tab.results);
}

/// Derive the `(instance, connection)` history key for rendering (mirrors the
/// update path's `session_key`).
fn session_view_key(session: &super::session::TabSession) -> (String, String) {
    let instance = session.instance.clone().unwrap_or_default();
    let connection = session
        .connection
        .clone()
        .or_else(|| session.connection_id.clone())
        .unwrap_or_default();
    (instance, connection)
}
