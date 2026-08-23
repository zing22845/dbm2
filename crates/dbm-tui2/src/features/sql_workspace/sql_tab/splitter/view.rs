//! Hit-testing and resize math for the SQL tab's top-level splitters (A:
//! editor/history; editor+history/results), plus the unified [`SqlSplitter`]
//! identity that also carries the History-internal detail/list splitter (B) for
//! drag routing. The app shell only passes a point and an area; all geometry
//! lives here or in the `history::splitter` feature.

use ratatui::layout::Rect;

use crate::common::view::splitter::hit;

use super::super::state::SqlTabState;
use super::super::msg::SqlTabMessage;
use super::super::layout::sql_tab_layout;
use crate::features::sql_workspace::sql_tab::history::splitter::view::{
    history_detail_splitter, history_zone_x,
};

/// Identifies which splitter a mouse position is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlSplitter {
    /// Horizontal splitter: editor+history row vs results.
    EditorResults,
    /// Vertical splitter: editor vs history in the top row (A).
    EditorHistory,
    /// Vertical splitter inside the History pane: detail preview vs list (B).
    HistoryDetail,
}

/// The splitter the position `(x, y)` is on, if any, in a static layout.
pub fn splitter_at(layout: &super::super::layout::SqlTabLayout, x: u16, y: u16) -> Option<SqlSplitter> {
    if hit(layout.h_splitter, x, y) {
        return Some(SqlSplitter::EditorResults);
    }
    if hit(layout.v_splitter, x, y) {
        return Some(SqlSplitter::EditorHistory);
    }
    None
}

/// Like [`splitter_at`], but when the History detail is visible the editor is
/// shrunk and the history zone is widened, so both the editor/history splitter
/// (A) and the internal detail/list splitter (B) move. `area` is the SQL-tab
/// body region.
pub fn splitter_at_with_detail(
    layout: &super::super::layout::SqlTabLayout,
    area: Rect,
    x: u16,
    y: u16,
    detail_visible: bool,
    detail_pane_width: u16,
) -> Option<SqlSplitter> {
    if !detail_visible {
        return splitter_at(layout, x, y);
    }
    // A moves to the widened zone's left edge (the editor is shrunk); B sits
    // just right of the detail.
    let zone_x = history_zone_x(area, layout, detail_pane_width);
    let editor_history = Rect::new(zone_x.saturating_sub(1), layout.v_splitter.y, 1, layout.v_splitter.height);
    if hit(editor_history, x, y) {
        return Some(SqlSplitter::EditorHistory);
    }
    if let Some(r) = history_detail_splitter(area, layout, true, detail_pane_width) {
        if hit(r, x, y) {
            return Some(SqlSplitter::HistoryDetail);
        }
    }
    None
}

/// Which splitter a drag starting at `(x, y)` in the SQL tab area hits, along
/// with the active tab's id.
pub fn sql_tab_splitter_at(
    state: &SqlTabState,
    area: Rect,
    x: u16,
    y: u16,
) -> Option<(usize, SqlSplitter)> {
    use crate::features::sql_workspace::sql_tab::session::session_view_key;
    use crate::features::sql_workspace::sql_tab::state::SqlFocus;
    let tab = state.active_tab()?;
    let body = Rect::new(area.x, area.y.saturating_add(1), area.width, area.height.saturating_sub(1));
    if body.width == 0 || body.height == 0 {
        return None;
    }
    let layout = sql_tab_layout(body, tab.splitter.editor_top_height, tab.splitter.history_pane_width);
    if layout.editor.width == 0 {
        return None;
    }
    let (instance, connection) = session_view_key(&tab.session);
    let detail_visible = tab.focus == SqlFocus::History
        && tab.history.store.entries(&instance, &connection).first().is_some();
    let splitter = if detail_visible {
        splitter_at_with_detail(
            &layout,
            body,
            x,
            y,
            true,
            tab.history.splitter.detail_pane_width,
        )
    } else {
        splitter_at(&layout, x, y)
    };
    // B is only draggable while History has focus (the detail is only shown then).
    let splitter = splitter.filter(|s| {
        !matches!(s, SqlSplitter::HistoryDetail) || tab.focus == SqlFocus::History
    });
    Some((tab.session.id, splitter?))
}

/// Resolve a drag of `splitter` to the new split value at `(x, y)` and build
/// the feature message. The feature computes its own geometry, so the app shell
/// never reasons about zone widths or `detail_pane_width`.
pub fn sql_tab_splitter_resize_msg(
    state: &SqlTabState,
    area: Rect,
    tab_id: usize,
    splitter: SqlSplitter,
    x: u16,
    y: u16,
) -> Option<SqlTabMessage> {
    use super::super::history::splitter::view::detail_width_for_x;
    let tab = state.tabs.get(state.index_of(tab_id)?)?;
    let body = Rect::new(area.x, area.y.saturating_add(1), area.width, area.height.saturating_sub(1));
    let layout = sql_tab_layout(body, tab.splitter.editor_top_height, tab.splitter.history_pane_width);
    if layout.editor.width == 0 {
        return None;
    }
    match splitter {
        SqlSplitter::EditorResults => {
            // Record the top-pane height in absolute rows (the exact row under
            // the pointer) — no percentage round-trip, so the splitter tracks
            // the pointer precisely. The stored rows are re-clamped to the
            // current track at layout time.
            let top_h = y.saturating_sub(layout.editor.y);
            Some(SqlTabMessage::SetEditorTopHeight { tab_id, height: top_h })
        }
        SqlSplitter::EditorHistory => {
            let right_edge = layout.history.right();
            let width = right_edge.saturating_sub(x);
            Some(SqlTabMessage::SetHistoryWidth { tab_id, width })
        }
        SqlSplitter::HistoryDetail => {
            let width = detail_width_for_x(body, &layout, tab.history.splitter.detail_pane_width, x);
            Some(SqlTabMessage::SetHistoryDetailWidth { tab_id, width })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::state::SqlTabState;

    #[test]
    fn hit_test_identifies_a_and_editor_results() {
        let area = Rect::new(0, 0, 120, 40);
        let layout = sql_tab_layout(area, 45, 24);
        assert_eq!(
            splitter_at(&layout, layout.v_splitter.x, layout.v_splitter.y),
            Some(SqlSplitter::EditorHistory)
        );
        assert_eq!(
            splitter_at(&layout, layout.h_splitter.x, layout.h_splitter.y),
            Some(SqlSplitter::EditorResults)
        );
        assert_eq!(splitter_at(&layout, 1, 1), None);
    }

    #[test]
    fn splitter_at_resolves_history_detail_when_open() {
        use crate::features::sql_workspace::sql_tab::session::session_view_key;
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;
        let mut state = SqlTabState::default();
        state.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        state.active_tab = Some(0);
        state.tabs[0].focus = SqlFocus::History;
        let (instance, connection) = session_view_key(&state.tabs[0].session);
        state.tabs[0]
            .history
            .store
            .record_success(&instance, &connection, "SELECT 1");
        let area = Rect::new(0, 0, 120, 40);
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(body, state.tabs[0].splitter.editor_top_height, state.tabs[0].splitter.history_pane_width);
        let detail_split =
            crate::features::sql_workspace::sql_tab::history::splitter::view::history_detail_splitter(
                body, &layout, true, state.tabs[0].history.splitter.detail_pane_width,
            )
            .unwrap();
        let (tab_id, s) = sql_tab_splitter_at(&state, area, detail_split.x, detail_split.y + 1).unwrap();
        assert_eq!(tab_id, state.tabs[0].session.id);
        assert_eq!(s, SqlSplitter::HistoryDetail);
    }

    #[test]
    fn resize_msg_builds_correct_messages() {
        let mut state = SqlTabState::default();
        state.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        state.active_tab = Some(0);
        let tab_id = state.tabs[0].session.id;
        let area = Rect::new(0, 0, 120, 40);
        let msg = sql_tab_splitter_resize_msg(&state, area, tab_id, SqlSplitter::EditorHistory, 60, 0).unwrap();
        assert!(matches!(msg, SqlTabMessage::SetHistoryWidth { tab_id: t, .. } if t == tab_id));
        let msg = sql_tab_splitter_resize_msg(&state, area, tab_id, SqlSplitter::HistoryDetail, 80, 0).unwrap();
        assert!(matches!(msg, SqlTabMessage::SetHistoryDetailWidth { tab_id: t, .. } if t == tab_id));
    }
}
