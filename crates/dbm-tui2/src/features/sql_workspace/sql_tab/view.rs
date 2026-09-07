//! `sql_tab` feature rendering.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::{Block, Paragraph};

use crate::common::layout::pane_scrollbar::ActiveScrollbar;
use crate::common::view::theme::Theme;

use super::editor::view as editor_view;
use super::history::view as history_view;
use super::layout::sql_tab_layout;
use super::results::view as results_view;
use super::session::{TabSession, session_view_key};
use super::state::{SqlFocus, SqlTabState};

/// Render the `sql_tab` feature: a tab bar plus the active tab's child panes.
/// The area is already inside the SQL workspace parent pane's border (the outer
/// " SQL Workspace " block is drawn by `sql_workspace/view.rs`). `focused`
/// tells whether the shell focus is on the SQL workspace; the active sub-pane's
/// border/title only lights up while the workspace itself is focused (matching
/// the original dbm). Returns the editor's hardware cursor when the editor
/// sub-pane holds focus (so the shell can place the terminal caret), else `None`,
/// plus the editor's rendered mouse hit area (for the pointer layer).
#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &SqlTabState,
    focused: bool,
    h_hover: bool,
    v_hover: bool,
    detail_hover: bool,
    h_drag: bool,
    v_drag: bool,
    detail_drag: bool,
    results_detail_hover: bool,
    results_detail_drag: bool,
    results_col_resize: Option<usize>,
    active_scrollbar: Option<ActiveScrollbar>,
) -> (
    Option<crate::common::editor::EditorHardwareCursor>,
    Option<usize>,
    Option<crate::common::editor::EditorMouseHitArea>,
) {
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
        return (None, None, None);
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
    let active_tab_idx = state.active_tab;
    super::tab::render(frame, theme, chunks[0], &sessions, &visible, active_tab_idx);

    let body_area = chunks[1];
    let Some(tab) = state.active_tab() else {
        // No open tab for the active connection: render an empty placeholder.
        frame.render_widget(Block::default().title("No open SQL tab"), body_area);
        return (None, None, None);
    };

    // Layout mirrors the original dbm `sql_tab_layout` (ui.rs §11): editor +
    // history on the top row, results below; the pane/splitter rects come from
    // the shared pure layout so the renderer and the run loop agree. A sub-pane
    // lights up only while the SQL workspace itself has shell focus.
    let editor_focused = focused && tab.focus == SqlFocus::Editor;
    let history_focused = focused && tab.focus == SqlFocus::History;
    let results_focused = focused && tab.focus == SqlFocus::Results;
    let layout = sql_tab_layout(
        body_area,
        tab.splitter.editor_top_height,
        tab.splitter.history_pane_width,
    );
    if layout.editor.width == 0 {
        // Area too small to split: show a single results pane.
        results_view::render(
            frame,
            theme,
            body_area,
            &tab.results,
            results_focused,
            results_detail_hover,
            results_detail_drag,
            results_col_resize,
            active_scrollbar,
        );
        return (None, None, None);
    }

    let (instance, connection) = session_view_key(&tab.session);
    // Mirror the original dbm: the detail preview is shown whenever the History
    // *pane* is focused (`tab.focus == History`), exactly like the original's
    // `detail_visible` (gated on `workspace_pane == History`, NOT the editor's
    // caret/shell focus). So focusing History with `H` pops the detail to the
    // left of the list immediately. The splitter can widen the detail, but it
    // is never absent while History is focused.
    let history_detail_visible = super::history::detail_visible(
        tab.focus == SqlFocus::History,
        &tab.history.list,
        &state.history_store,
        &instance,
        &connection,
    );

    // When the detail is visible it extends the history zone to the left,
    // eating into the editor's width (mirrors original `history_zone_width`).
    // Compute that zone first so the editor below can be shrunk to make room,
    // instead of the detail painting over it.
    let mut editor_area = layout.editor;
    // The editor/history vertical splitter position. Normally it is the shared
    // layout's `v_splitter`, but when the detail is visible the history zone
    // grows leftward and the editor shrinks, so the splitter must move to the
    // new editor right edge (history_zone.x - 1) rather than sitting inside the
    // history pane and drawing a bogus second line.
    let mut editor_history_splitter_x = layout.v_splitter.x;
    let history_zone = if history_detail_visible {
        let zone_x = super::history::splitter::layout::history_zone_x(
            area,
            &layout,
            tab.history.splitter.detail_pane_width,
        );
        // The zone must leave the editor its minimum width, or a very wide
        // history pane (from the splitter drag) squeezes the editor to zero and
        // hangs edtui's wrapped render.
        let max_zone_w = area
            .width
            .saturating_sub(
                crate::features::sql_workspace::sql_tab::splitter::state::MIN_SQL_PANE_WIDTH,
            )
            .max(1);
        let zone_w = super::history::splitter::layout::history_zone_width(
            &layout,
            tab.history.splitter.detail_pane_width,
        )
        .min(max_zone_w);
        // Reconcile the stored splitter values against the geometry this frame
        // actually renders. When the History detail is visible the list pane
        // must equal `zone - border - detail - splitter`; any drift here means
        // the stored width and the rendered geometry disagree (which would cause
        // redundant repaints at the drag limit), so surface it as a warning.
        {
            use crate::features::sql_workspace::sql_tab::history::splitter::state::clamp_detail_pane_width;
            let d = clamp_detail_pane_width(tab.history.splitter.detail_pane_width);
            let stored_list = tab.splitter.history_pane_width;
            // The History border eats 2 columns before the detail/list split.
            let rendered_list = zone_w.saturating_sub(2).saturating_sub(d).saturating_sub(1);
            if stored_list != rendered_list {
                tracing::warn!(
                    zone_x,
                    zone_w,
                    max_zone_w,
                    detail = d,
                    stored_list,
                    rendered_list,
                    "sql_tab zone reconcile: stored list != rendered list"
                );
            }
        }
        // The splitter sits just left of the widened history zone.
        editor_history_splitter_x = zone_x.saturating_sub(1);
        // Shrink the editor to end where the detail zone begins (minus the
        // vertical splitter between editor and history).
        editor_area = Rect::new(
            editor_area.x,
            editor_area.y,
            zone_x
                .saturating_sub(editor_area.x)
                .saturating_sub(layout.v_splitter.width)
                .max(1),
            editor_area.height,
        );
        Some(Rect::new(
            zone_x,
            layout.history.y,
            zone_w,
            layout.history.height,
        ))
    } else {
        None
    };

    // Only the focused editor sub-pane exposes its caret to the shell. The
    // rendered mouse hit area is captured either way: the pointer layer needs
    // it to route clicks/selection even while another sub-pane has focus.
    let db = tab.session.database.as_deref();
    let schema = tab.session.schema.as_deref();
    let (rendered_cursor, editor_mouse_area) = editor_view::render(
        frame,
        theme,
        editor_area,
        &tab.editor,
        editor_focused,
        tab.complete_table_names,
        db,
        schema,
        active_scrollbar,
    );
    let cursor = if editor_focused {
        rendered_cursor
    } else {
        None
    };

    // The History feature owns both the list and the detail under a single
    // border; pass the full history zone and let it split internally.
    let history_v_scroll = if let Some(history_zone) = history_zone {
        tracing::debug!(
            ?history_zone,
            detail_w = tab.history.splitter.detail_pane_width,
            "render: begin history_view"
        );
        history_view::render(
            frame,
            theme,
            history_zone,
            &tab.history,
            &state.history_store,
            &instance,
            &connection,
            history_focused,
            true,
            tab.history.splitter.detail_pane_width,
            detail_hover,
            detail_drag,
            active_scrollbar,
        )
    } else {
        history_view::render(
            frame,
            theme,
            layout.history,
            &tab.history,
            &state.history_store,
            &instance,
            &connection,
            history_focused,
            false,
            tab.history.splitter.detail_pane_width,
            detail_hover,
            detail_drag,
            active_scrollbar,
        )
    };

    results_view::render(
        frame,
        theme,
        layout.results,
        &tab.results,
        results_focused,
        results_detail_hover,
        results_detail_drag,
        results_col_resize,
        active_scrollbar,
    );

    // Draw the two draggable splitter strips. The editor/history splitter is
    // drawn at the (possibly shifted) editor right edge so it never lands
    // inside the history pane when the detail is visible.
    let v_splitter_rect = Rect::new(
        editor_history_splitter_x,
        layout.v_splitter.y,
        layout.v_splitter.width,
        layout.v_splitter.height,
    );
    super::splitter::view::render(
        frame,
        layout.h_splitter,
        v_splitter_rect,
        h_hover,
        h_drag,
        v_hover,
        v_drag,
    );

    let hw_cursor = if editor_focused { cursor } else { None };
    (hw_cursor, history_v_scroll, editor_mouse_area)
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
    fn splitter_hover_renders_at_correct_position() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Color;

        let mut state = state_with_tabs(1);
        state.active_tab = Some(0);
        let theme = crate::common::view::theme::default();
        let area = Rect::new(0, 0, 120, 40);

        // Test 1: hover on vertical splitter should light up vertical, not horizontal
        {
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            terminal
                .draw(|frame| {
                    let _ = render(
                        frame, &theme, area, &state, true, false, true, false, false, false, false,
                        false, false, None, None,
                    );
                })
                .unwrap();
            let buf = terminal.backend().buffer();
            let layout = crate::features::sql_workspace::sql_tab::layout::sql_tab_layout(
                Rect::new(0, 1, 120, 39),
                state.tabs[0].splitter.editor_top_height,
                state.tabs[0].splitter.history_pane_width,
            );
            let vx = layout.v_splitter.x;
            let vy = layout.v_splitter.y;
            let hx = layout.h_splitter.x;
            let hy = layout.h_splitter.y;

            let v_cell = buf.cell((vx, vy)).unwrap();
            let h_cell = buf.cell((hx, hy)).unwrap();

            assert_eq!(
                v_cell.fg,
                Color::Cyan,
                "vertical splitter at ({}, {}) should be Cyan when hovered, got {:?}",
                vx,
                vy,
                v_cell.fg
            );
            assert_eq!(
                h_cell.fg,
                Color::Rgb(55, 55, 60),
                "horizontal splitter at ({}, {}) should be DIM when NOT hovered, got {:?}",
                hx,
                hy,
                h_cell.fg
            );
        }

        // Test 2: hover on horizontal splitter should light up horizontal, not vertical
        {
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            terminal
                .draw(|frame| {
                    let _ = render(
                        frame, &theme, area, &state, true, true, false, false, false, false, false,
                        false, false, None, None,
                    );
                })
                .unwrap();
            let buf = terminal.backend().buffer();
            let layout = crate::features::sql_workspace::sql_tab::layout::sql_tab_layout(
                Rect::new(0, 1, 120, 39),
                state.tabs[0].splitter.editor_top_height,
                state.tabs[0].splitter.history_pane_width,
            );
            let vx = layout.v_splitter.x;
            let vy = layout.v_splitter.y;
            let hx = layout.h_splitter.x;
            let hy = layout.h_splitter.y;

            let v_cell = buf.cell((vx, vy)).unwrap();
            let h_cell = buf.cell((hx, hy)).unwrap();

            assert_eq!(
                h_cell.fg,
                Color::Cyan,
                "horizontal splitter at ({}, {}) should be Cyan when hovered, got {:?}",
                hx,
                hy,
                h_cell.fg
            );
            assert_eq!(
                v_cell.fg,
                Color::Rgb(55, 55, 60),
                "vertical splitter at ({}, {}) should be DIM when NOT hovered, got {:?}",
                vx,
                vy,
                v_cell.fg
            );
        }
    }

    #[test]
    fn render_does_not_panic_at_extreme_split_widths() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let theme = crate::common::view::theme::default();
        let area = Rect::new(0, 0, 120, 40);
        // After dragging the splitters, detail/history widths can reach their
        // extremes; the renderer must not panic.
        for detail_w in [24u16, 40, 72] {
            for history_w in [12u16, 24, 60] {
                let mut state = state_with_tabs(1);
                state.active_tab = Some(0);
                state.tabs[0].focus =
                    crate::features::sql_workspace::sql_tab::state::SqlFocus::History;
                state.tabs[0].splitter.history_pane_width = history_w;
                state.tabs[0].history.splitter.detail_pane_width = detail_w;
                let (instance, connection) = session_view_key(&state.tabs[0].session);
                state
                    .history_store
                    .record_success(&instance, &connection, "SELECT * FROM users");
                let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    terminal
                        .draw(|frame| {
                            let _ = render(
                                frame, &theme, area, &state, true, false, false, false, false,
                                false, false, false, false, None, None,
                            );
                        })
                        .unwrap();
                }));
                assert!(
                    r.is_ok(),
                    "render panicked at detail_w={detail_w}, history_w={history_w}"
                );
            }
        }
    }

    #[test]
    fn sql_tab_render_does_not_hang_with_editor_text_at_history_w84() {
        // The exact drag-triggered state that used to hang the full app render:
        // History focused (detail expanded), history pane width 84 (set by the
        // editor/history splitter drag), and real SQL text in the editor.
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut state = state_with_tabs(1);
        state.active_tab = Some(0);
        state.tabs[0].focus = crate::features::sql_workspace::sql_tab::state::SqlFocus::History;
        state.tabs[0].splitter.history_pane_width = 84;
        state.tabs[0].history.splitter.detail_pane_width = 40;
        state.tabs[0].editor =
            crate::features::sql_workspace::sql_tab::editor::state::EditorState::with_sql(
                "SELECT * FROM \"测试表\" WHERE id = 1 AND name ILIKE '%foo%' ORDER BY created_at DESC",
            );
        let (instance, connection) = session_view_key(&state.tabs[0].session);
        state
            .history_store
            .record_success(&instance, &connection, "SELECT * FROM users");
        let theme = crate::common::view::theme::default();
        // Match the real workspace width (80% of 160 minus the border) so the
        // editor is actually squeezed by the 84-wide history zone.
        let area = Rect::new(0, 0, 126, 40);
        let mut terminal = Terminal::new(TestBackend::new(126, 40)).unwrap();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let _ = render(
                        frame, &theme, area, &state, true, false, false, false, false, false,
                        false, false, false, None, None,
                    );
                })
                .unwrap();
        }));
        assert!(
            r.is_ok(),
            "sql_tab render hung/panicked at history_w=84 with editor text"
        );
    }
}
