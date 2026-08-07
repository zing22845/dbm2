//! `sql_tab` feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::common::view::splitter::{SplitOrientation, draw};
use crate::common::view::theme::Theme;

use super::layout::sql_tab_layout;
use super::session::TabSession;
use super::state::SqlTabState;
use super::editor::view as editor_view;
use super::history::view as history_view;
use super::results::view as results_view;

/// Render the `sql_tab` feature: a tab bar plus the active tab's child panes.
/// The area is already inside the SQL workspace parent pane's border (the outer
/// " SQL Workspace " block is drawn by `sql_workspace/view.rs`).
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &SqlTabState) {
    // No connection tab open: show an empty-state hint and no tab bar, mirroring
    // the original dbm's `workspace_empty_hint` (no phantom "sql 0" tab, no
    // editor / history / results panes).
    if state.tabs.is_empty() {
        let p = theme.palette();
        let hint = crate::common::view::hints::sql_workspace_empty_hint();
        let para = Paragraph::new(Span::styled(
            hint,
            Style::default().fg(p.muted),
        ));
        frame.render_widget(para, area);
        return;
    }

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

    let body_area = chunks[1];
    let Some(tab) = state.tabs.get(state.active_tab) else {
        // No tab is open: render an empty placeholder in the body.
        frame.render_widget(Block::default().title("No open SQL tab"), body_area);
        return;
    };

    // Layout mirrors the original dbm `sql_tab_layout` (ui.rs §11): editor +
    // history on the top row, results below; the pane/splitter rects come from
    // the shared pure layout so the renderer and the run loop agree.
    let layout = sql_tab_layout(body_area, tab.split_ratio, tab.history_pane_width);
    if layout.editor.width == 0 {
        // Area too small to split: show a single results pane.
        results_view::render(frame, theme, body_area, &tab.results);
        return;
    }

    editor_view::render(frame, theme, layout.editor, &tab.editor);

    let (instance, connection) = session_view_key(&tab.session);
    history_view::render(frame, theme, layout.history, &tab.history, &instance, &connection);

    results_view::render(frame, theme, layout.results, &tab.results);

    // Draw the two draggable splitter strips.
    draw(frame, layout.h_splitter, SplitOrientation::Horizontal, false, false);
    draw(frame, layout.v_splitter, SplitOrientation::Vertical, false, false);
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
