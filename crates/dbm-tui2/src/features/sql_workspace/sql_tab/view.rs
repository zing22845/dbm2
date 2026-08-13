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
use super::state::{SqlFocus, SqlTabState};
use super::editor::view as editor_view;

/// The action produced by a mouse click inside the SQL workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlClickAction {
    /// Focus the clicked sub-pane (editor / history / results).
    FocusSubPane(SqlFocus),
    /// Activate the clicked tab, given its visible-tab offset.
    ActivateTab(usize),
}

/// Hit-test a click at `(x, y)` inside the SQL workspace's tab-bar + body
/// region (`area`). A click on the top tab bar activates that tab; a click in
/// the body focuses the sub-pane (editor / history / results) under the cursor.
/// Returns `None` for clicks outside any interactive region (splitters,
/// footer, empty areas).
pub fn sql_workspace_click(
    state: &SqlTabState,
    area: ratatui::layout::Rect,
    x: u16,
    y: u16,
) -> Option<SqlClickAction> {
    // The tab bar is the single top row; the child panes are below it.
    let tab_bar = ratatui::layout::Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    let body = ratatui::layout::Rect {
        x: area.x,
        y: area.y.saturating_add(1),
        width: area.width,
        height: area.height.saturating_sub(1),
    };

    // Click on the tab bar: activate that tab.
    if y == tab_bar.y {
        let sessions: Vec<TabSession> = state.tabs.iter().map(|t| t.session.clone()).collect();
        let visible = state.visible_tab_indices();
        return super::tab::tab_at(tab_bar, &sessions, &visible, x, y).map(SqlClickAction::ActivateTab);
    }

    // Click in the body: focus the sub-pane under the cursor.
    let Some(tab) = state.tabs.get(state.active_tab) else {
        return None;
    };
    if body.width == 0 || body.height == 0 {
        return None;
    }
    let layout = sql_tab_layout(body, tab.split_ratio, tab.history_pane_width);
    if layout.editor.width == 0 {
        return None;
    }
    let focus = if contains(layout.editor, x, y) {
        SqlFocus::Editor
    } else if contains(layout.history, x, y) {
        SqlFocus::History
    } else if contains(layout.results, x, y) {
        SqlFocus::Results
    } else {
        return None; // splitter or footer
    };
    Some(SqlClickAction::FocusSubPane(focus))
}

fn contains(r: ratatui::layout::Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x.saturating_add(r.width) && y >= r.y && y < r.y.saturating_add(r.height)
}
use super::history::view as history_view;
use super::results::view as results_view;

/// Render the `sql_tab` feature: a tab bar plus the active tab's child panes.
/// The area is already inside the SQL workspace parent pane's border (the outer
/// " SQL Workspace " block is drawn by `sql_workspace/view.rs`). `focused`
/// tells whether the shell focus is on the SQL workspace; the active sub-pane's
/// border/title only lights up while the workspace itself is focused (matching
/// the original dbm). Returns the editor's hardware cursor when the editor
/// sub-pane holds focus (so the shell can place the terminal caret), else `None`.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &SqlTabState,
    focused: bool,
) -> Option<crate::common::editor::EditorHardwareCursor> {
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
        return None;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tab bar
            Constraint::Min(0),    // child panes
        ])
        .split(area);

    // Tab bar: only render tabs belonging to the active connection.
    let visible = state.visible_tab_indices();
    let sessions: Vec<TabSession> = state.tabs.iter().map(|t| t.session.clone()).collect();
    super::tab::render(frame, theme, chunks[0], &sessions, &visible, Some(state.active_tab));

    let body_area = chunks[1];
    let Some(tab) = state.tabs.get(state.active_tab) else {
        // No tab is open: render an empty placeholder in the body.
        frame.render_widget(Block::default().title("No open SQL tab"), body_area);
        return None;
    };

    // Layout mirrors the original dbm `sql_tab_layout` (ui.rs §11): editor +
    // history on the top row, results below; the pane/splitter rects come from
    // the shared pure layout so the renderer and the run loop agree. A sub-pane
    // lights up only while the SQL workspace itself has shell focus.
    let editor_focused = focused && tab.focus == SqlFocus::Editor;
    let history_focused = focused && tab.focus == SqlFocus::History;
    let results_focused = focused && tab.focus == SqlFocus::Results;
    let layout = sql_tab_layout(body_area, tab.split_ratio, tab.history_pane_width);
    if layout.editor.width == 0 {
        // Area too small to split: show a single results pane.
        results_view::render(frame, theme, body_area, &tab.results, results_focused);
        return None;
    }

    // Only the focused editor sub-pane exposes its caret to the shell.
    let cursor = if editor_focused {
        editor_view::render(frame, theme, layout.editor, &tab.editor, editor_focused)
    } else {
        editor_view::render(frame, theme, layout.editor, &tab.editor, editor_focused);
        None
    };

    let (instance, connection) = session_view_key(&tab.session);
    history_view::render(
        frame,
        theme,
        layout.history,
        &tab.history,
        &instance,
        &connection,
        history_focused,
    );

    results_view::render(frame, theme, layout.results, &tab.results, results_focused);

    // Draw the two draggable splitter strips.
    draw(frame, layout.h_splitter, SplitOrientation::Horizontal, false, false);
    draw(frame, layout.v_splitter, SplitOrientation::Vertical, false, false);

    if editor_focused { cursor } else { None }
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn state_with_tabs(count: usize) -> SqlTabState {
        let mut s = SqlTabState::default();
        for _ in 0..count {
            s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None);
        }
        s
    }

    #[test]
    fn click_tab_bar_activates_tab() {
        let state = state_with_tabs(2);
        // area at x=0,y=0; tab bar is row 0. Click on the first tab.
        let area = Rect::new(0, 0, 80, 20);
        assert_eq!(
            sql_workspace_click(&state, area, 1, 0),
            Some(SqlClickAction::ActivateTab(0))
        );
        // Second tab is just past "<SQL 1> " (8 chars).
        assert_eq!(
            sql_workspace_click(&state, area, 9, 0),
            Some(SqlClickAction::ActivateTab(1))
        );
    }

    #[test]
    fn click_body_focuses_subpane() {
        let state = state_with_tabs(1);
        // Body is rows 1..; the editor occupies the left of the top row.
        let area = Rect::new(0, 0, 120, 40);
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(body, state.tabs[0].split_ratio, state.tabs[0].history_pane_width);
        // Click inside the editor region -> focus editor.
        let p = (layout.editor.x + 1, layout.editor.y + 1);
        assert_eq!(
            sql_workspace_click(&state, area, p.0, p.1),
            Some(SqlClickAction::FocusSubPane(SqlFocus::Editor))
        );
        // Click inside the history region -> focus history.
        let p = (layout.history.x + 1, layout.history.y + 1);
        assert_eq!(
            sql_workspace_click(&state, area, p.0, p.1),
            Some(SqlClickAction::FocusSubPane(SqlFocus::History))
        );
    }

    #[test]
    fn click_empty_or_splitter_returns_none() {
        let state = state_with_tabs(1);
        let area = Rect::new(0, 0, 120, 40);
        // Click on the splitter row between top row and results -> none.
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(body, state.tabs[0].split_ratio, state.tabs[0].history_pane_width);
        let p = (body.x + 1, layout.h_splitter.y);
        assert_eq!(sql_workspace_click(&state, area, p.0, p.1), None);
        // Click in the body below results (should be inside results actually);
        // instead assert a click outside the whole area returns none.
        assert_eq!(sql_workspace_click(&state, area, 500, 500), None);
    }
}
