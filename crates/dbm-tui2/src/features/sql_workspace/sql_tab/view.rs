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
    /// Close the open context picker (click outside it).
    CloseContextPicker,
    /// Click the editor header's context segment: open the context picker
    /// focused on `column` (the `· {db}` segment → Database, the `› {schema}`
    /// remainder → Schema).
    OpenContextPicker(crate::features::sql_workspace::sql_tab::editor::context_picker::state::PickerColumn),
    /// Click a picker row: move the cursor to that row (switching column);
    /// `double` additionally applies the selection.
    ContextPickerHit {
        column: crate::features::sql_workspace::sql_tab::editor::context_picker::state::PickerColumn,
        cursor: usize,
        double: bool,
    },
    /// Click inside a picker column area (not on a row): switch the focused column.
    ContextPickerColumn(crate::features::sql_workspace::sql_tab::editor::context_picker::state::PickerColumn),
}

/// Hit-test a click at `(x, y)` inside the SQL workspace's tab-bar + body
/// region (`area`). A click on the top tab bar activates that tab; a click in
/// the body focuses the sub-pane (editor / history / results) under the cursor.
/// When the context picker is open, clicks on its rows/columns operate on it and
/// clicks outside it close it (mirroring the original dbm). Returns `None` for
/// clicks outside any interactive region (splitters, footer, empty areas).
pub fn sql_workspace_click(
    state: &SqlTabState,
    area: ratatui::layout::Rect,
    x: u16,
    y: u16,
    is_double_click: bool,
) -> Option<SqlClickAction> {
    use crate::features::sql_workspace::sql_tab::editor::context_picker::state::PickerColumn;
    use crate::features::sql_workspace::sql_tab::editor::context_picker::view as cp_view;

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
    let tab = state.active_tab()?;
    if body.width == 0 || body.height == 0 {
        return None;
    }
    let layout = sql_tab_layout(body, tab.split_ratio, tab.history_pane_width);
    if layout.editor.width == 0 {
        return None;
    }

    // When the picker is open, clicks inside it operate on it; clicks outside
    // it close it (the picker owns the editor sub-pane region).
    let picker_open = tab.editor.context_picker.open;
    if let Some(picker_area) = editor_view::context_picker_area(layout.editor, picker_open) {
        if contains(picker_area, x, y) {
            // A click on a visible row moves the cursor there (and, on a double
            // click, applies the selection).
            if let Some((column, cursor)) = cp_view::row_hit_at(picker_area, &tab.editor.context_picker, x, y) {
                return Some(SqlClickAction::ContextPickerHit {
                    column,
                    cursor,
                    double: is_double_click,
                });
            }
            // A click on a column's area (not a row) switches that column.
            let (db_rect, schema_rect) = cp_view::column_rects(picker_area);
            if contains(db_rect, x, y) {
                return Some(SqlClickAction::ContextPickerColumn(PickerColumn::Database));
            }
            if contains(schema_rect, x, y) {
                return Some(SqlClickAction::ContextPickerColumn(PickerColumn::Schema));
            }
            return None;
        }
        // A click in the SQL body outside the open picker closes it.
        return Some(SqlClickAction::CloseContextPicker);
    }

    // A click on the editor's title bar context segment opens the context
    // picker, focused on the column matching the clicked chip. The trigger is
    // always present (even without a chosen database), matching the original
    // dbm.
    if y == layout.editor.y {
        let (db_rect, full_rect) = editor_view::context_trigger_rects(
            layout.editor,
            tab.editor.editor.mode,
            tab.session.database.as_deref(),
            tab.session.schema.as_deref(),
        );
        if contains(full_rect, x, y) {
            let column = if contains(db_rect, x, y) {
                PickerColumn::Database
            } else {
                PickerColumn::Schema
            };
            return Some(SqlClickAction::OpenContextPicker(column));
        }
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
    // No tab open for the active connection: show an empty-state hint and no
    // tab bar, mirroring the original dbm's `workspace_empty_hint` (no phantom
    // "sql 0" tab, no editor / history / results panes). The hint differs based
    // on whether a connection is active at all.
    if state.active_connection_is_empty() {
        let p = theme.palette();
        let hint = crate::common::view::hints::sql_workspace_empty_hint(
            state.active_connection().is_some(),
        );
        let para = Paragraph::new(Span::styled(hint, Style::default().fg(p.muted)));
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
    super::tab::render(frame, theme, chunks[0], &sessions, &visible, state.active_tab);

    let body_area = chunks[1];
    let Some(tab) = state.active_tab() else {
        // No open tab for the active connection: render an empty placeholder.
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
    let db = tab.session.database.as_deref();
    let schema = tab.session.schema.as_deref();
    let cursor = if editor_focused {
        editor_view::render(
            frame,
            theme,
            layout.editor,
            &tab.editor,
            editor_focused,
            tab.complete_table_names,
            db,
            schema,
        )
    } else {
        editor_view::render(
            frame,
            theme,
            layout.editor,
            &tab.editor,
            editor_focused,
            tab.complete_table_names,
            db,
            schema,
        );
        None
    };

    let (instance, connection) = session_view_key(&tab.session);
    // Mirroring the original dbm, the History pane shows both the list and the
    // detail preview of the selected entry whenever the pane is focused (the
    // splitter can widen the detail, but it is never absent). When not focused
    // we keep the list-only view to save space.
    if history_focused {
        let detail_w = crate::features::sql_workspace::sql_tab::history::detail::DEFAULT_DETAIL_PANE_WIDTH;
        if layout.history.width > detail_w + 4 {
            let halves = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Horizontal)
                .constraints([
                    ratatui::layout::Constraint::Min(1),
                    ratatui::layout::Constraint::Length(detail_w),
                ])
                .split(layout.history);
            history_view::render(
                frame,
                theme,
                halves[0],
                &tab.history,
                &instance,
                &connection,
                history_focused,
            );
            if let Some(sql) = tab.history.selected_entry(&instance, &connection) {
                let mut content = Rect::default();
                let mut v_bar = Rect::default();
                crate::features::sql_workspace::sql_tab::history::detail::draw_history_detail(
                    frame,
                    halves[1],
                    &sql,
                    &tab.history.detail,
                    &tab.history.search,
                    theme,
                    &mut content,
                    &mut v_bar,
                );
            }
        } else {
            history_view::render(
                frame,
                theme,
                layout.history,
                &tab.history,
                &instance,
                &connection,
                history_focused,
            );
        }
    } else {
        history_view::render(
            frame,
            theme,
            layout.history,
            &tab.history,
            &instance,
            &connection,
            history_focused,
        );
    }

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
            s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        }
        s
    }

    #[test]
    fn click_tab_bar_activates_tab() {
        let state = state_with_tabs(2);
        // area at x=0,y=0; tab bar is row 0. Click on the first tab.
        let area = Rect::new(0, 0, 80, 20);
        assert_eq!(
            sql_workspace_click(&state, area, 1, 0, false),
            Some(SqlClickAction::ActivateTab(0))
        );
        // Second tab is just past "<SQL 1> " (8 chars).
        assert_eq!(
            sql_workspace_click(&state, area, 9, 0, false),
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
            sql_workspace_click(&state, area, p.0, p.1, false),
            Some(SqlClickAction::FocusSubPane(SqlFocus::Editor))
        );
        // Click inside the history region -> focus history.
        let p = (layout.history.x + 1, layout.history.y + 1);
        assert_eq!(
            sql_workspace_click(&state, area, p.0, p.1, false),
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
        assert_eq!(sql_workspace_click(&state, area, p.0, p.1, false), None);
        // Click in the body below results (should be inside results actually);
        // instead assert a click outside the whole area returns none.
        assert_eq!(sql_workspace_click(&state, area, 500, 500, false), None);
    }

    #[test]
    fn click_header_trigger_opens_picker_focused_on_column() {
        use crate::features::sql_workspace::sql_tab::editor::context_picker::state::PickerColumn;
        let mut state = state_with_tabs(1);
        state.tabs[0].session.database = Some("mydb".into());
        state.tabs[0].session.schema = Some("public".into());
        let area = Rect::new(0, 0, 120, 40);
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(body, state.tabs[0].split_ratio, state.tabs[0].history_pane_width);
        // Click the `· mydb` segment -> focus Database column.
        let (db_rect, _full) = editor_view::context_trigger_rects(
            layout.editor,
            state.tabs[0].editor.editor.mode,
            Some("mydb"),
            Some("public"),
        );
        assert_eq!(
            sql_workspace_click(&state, area, db_rect.x + 1, db_rect.y, false),
            Some(SqlClickAction::OpenContextPicker(PickerColumn::Database))
        );
        // Click the `› public` remainder -> focus Schema column.
        let (_db, full) = editor_view::context_trigger_rects(
            layout.editor,
            state.tabs[0].editor.editor.mode,
            Some("mydb"),
            Some("public"),
        );
        assert_eq!(
            sql_workspace_click(&state, area, full.x + full.width - 1, full.y, false),
            Some(SqlClickAction::OpenContextPicker(PickerColumn::Schema))
        );
    }

    #[test]
    fn picker_row_click_and_column_click_and_outside_close() {
        use crate::features::sql_workspace::sql_tab::editor::context_picker::{
            state::{CachedList, ContextPickerState, PickerColumn},
            view as cp_view,
        };
        let mut state = state_with_tabs(1);
        state.tabs[0].session.database = Some("mydb".into());
        state.tabs[0].session.schema = Some("public".into());
        state.tabs[0].editor.context_picker = ContextPickerState::open(
            PickerColumn::Database,
            "inst".into(),
            "conn".into(),
            "mydb".into(),
            "public".into(),
        );
        {
            let cp = &mut state.tabs[0].editor.context_picker;
            cp.databases = CachedList::Ready(vec!["a".into(), "b".into(), "c".into()]);
            cp.schemas = CachedList::Ready(vec!["public".into()]);
        }
        let area = Rect::new(0, 0, 120, 40);
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(body, state.tabs[0].split_ratio, state.tabs[0].history_pane_width);
        let picker_area = editor_view::context_picker_area(layout.editor, true).unwrap();
        let (db_rect, schema_rect) = cp_view::column_rects(picker_area);

        // Click a database row (inside the db column body) -> cursor hit.
        let row_y = db_rect.y + 1;
        let hit = sql_workspace_click(&state, area, db_rect.x + 2, row_y, false);
        assert!(matches!(
            hit,
            Some(SqlClickAction::ContextPickerHit {
                column: PickerColumn::Database,
                cursor,
                double: false
            }) if cursor == 0
        ));

        // Click the schema column area (its title row, not a row body) -> switch column.
        assert_eq!(
            sql_workspace_click(&state, area, schema_rect.x + 2, schema_rect.y, false),
            Some(SqlClickAction::ContextPickerColumn(PickerColumn::Schema))
        );

        // Click outside the picker (e.g. the history pane) -> close it.
        assert_eq!(
            sql_workspace_click(&state, area, layout.history.x + 1, layout.history.y + 1, false),
            Some(SqlClickAction::CloseContextPicker)
        );
    }

    #[test]
    fn connection_a_open_picker_does_not_leak_into_connection_b() {
        use crate::features::sql_workspace::sql_tab::editor::context_picker::state::{
            ContextPickerState, PickerColumn,
        };
        // Two tabs: A (index 0) has its context picker left open; B (index 1)
        // is the active connection's tab and never opened a picker.
        let mut state = SqlTabState::default();
        state.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        state.open_connection_tab("inst".into(), "c2".into(), "id2".into(), None, None, None);
        state.tabs[0].session.database = Some("dbA".into());
        state.tabs[0].session.schema = Some("public".into());
        state.tabs[0].editor.context_picker = ContextPickerState::open(
            PickerColumn::Database,
            "inst".into(),
            "c1".into(),
            "dbA".into(),
            "public".into(),
        );
        state.tabs[1].session.database = Some("dbB".into());
        state.tabs[1].session.schema = Some("public".into());
        // B is active.
        state.active_tab = Some(1);

        let area = Rect::new(0, 0, 120, 40);
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(body, state.tabs[1].split_ratio, state.tabs[1].history_pane_width);

        // Clicking B's context trigger opens B's picker (B's picker is closed,
        // so this is NOT treated as an outside-click-close).
        let (_db, full) = editor_view::context_trigger_rects(
            layout.editor,
            state.tabs[1].editor.editor.mode,
            Some("dbB"),
            Some("public"),
        );
        assert_eq!(
            sql_workspace_click(&state, area, full.x + full.width - 1, full.y, false),
            Some(SqlClickAction::OpenContextPicker(PickerColumn::Schema))
        );

        // Clicking B's editor body focuses the editor (not CloseContextPicker,
        // because B's picker is closed).
        assert_eq!(
            sql_workspace_click(&state, area, layout.editor.x + 2, layout.editor.y + 3, false),
            Some(SqlClickAction::FocusSubPane(SqlFocus::Editor))
        );
    }

    #[test]
    fn connection_without_database_can_still_open_context_picker() {
        use crate::features::sql_workspace::sql_tab::editor::context_picker::state::PickerColumn;
        // A connection whose tab has no database yet (opened straight from the
        // instances tree) must still be able to open its own context picker —
        // the picker's whole purpose is to choose a database. Previously the
        // header trigger was hidden when `database` was empty, so the click did
        // nothing and only connections that had already applied a context could
        // open a picker.
        let state = state_with_tabs(1);
        assert_eq!(state.tabs[0].session.database, None);
        let area = Rect::new(0, 0, 120, 40);
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(body, state.tabs[0].split_ratio, state.tabs[0].history_pane_width);
        let (_db, full) = editor_view::context_trigger_rects(
            layout.editor,
            state.tabs[0].editor.editor.mode,
            None,
            None,
        );
        // The `› …` remainder focuses the Schema column (database not chosen yet).
        assert_eq!(
            sql_workspace_click(&state, area, full.x + full.width - 1, full.y, false),
            Some(SqlClickAction::OpenContextPicker(PickerColumn::Schema))
        );
    }
}
