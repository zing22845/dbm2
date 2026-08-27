//! Hit-testing and resize math for the SQL tab's top-level splitters (A:
//! editor/history; editor+history/results), plus the unified [`SqlSplitter`]
//! identity that also carries the History-internal detail/list splitter (B) for
//! drag routing. The app shell only passes a point and an area; all geometry
//! lives here or in the `history::splitter` feature.

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::common::view::splitter::{draw, hit, SplitOrientation};

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
    /// Vertical splitter inside the Results pane: list vs detail preview.
    ResultsDetail,
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
    // The horizontal splitter (editor+history row vs results) is unaffected
    // by the detail panel — its row stays at the same track position — so
    // the base layout's h_splitter rect is still valid for hit-testing.
    if hit(layout.h_splitter, x, y) {
        return Some(SqlSplitter::EditorResults);
    }
    // A moves to the widened zone's left edge (the editor is shrunk); B sits
    // just right of the detail.
    let zone_x = history_zone_x(area, layout, detail_pane_width);
    let editor_history = Rect::new(zone_x.saturating_sub(1), layout.v_splitter.y, 1, layout.v_splitter.height);
    if hit(editor_history, x, y) {
        return Some(SqlSplitter::EditorHistory);
    }
    if let Some(r) = history_detail_splitter(area, layout, true, detail_pane_width)
        && hit(r, x, y) {
            return Some(SqlSplitter::HistoryDetail);
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
        && !state.history_store.entries(&instance, &connection).is_empty();
    let mut splitter = if detail_visible {
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
    // History Detail (B) is only draggable while History has focus.
    splitter = splitter.filter(|s| {
        !matches!(s, SqlSplitter::HistoryDetail) || tab.focus == SqlFocus::History
    });
    // Results-internal detail/list splitter (C) — independent of History, hit
    // only when results detail is open AND Results has focus.
    if splitter.is_none() && tab.results.detail_open && tab.focus == SqlFocus::Results {
        use crate::features::sql_workspace::sql_tab::results::splitter::view::results_detail_splitter;
        if let Some(r) = results_detail_splitter(
            layout.results,
            true,
            tab.results.splitter.detail_pane_width,
        ) && hit(r, x, y)
        {
            splitter = Some(SqlSplitter::ResultsDetail);
        }
    }
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
            // the pointer precisely. Clamp to the valid `[20%, 80%]` row range
            // of the current track so a pointer dragged beyond the boundary
            // leaves the stored height (and thus the rendered split) unchanged:
            // the drag then stops emitting redundant repaints, which would
            // otherwise inflate the waste metric.
            let track_h = layout.results.bottom().saturating_sub(layout.editor.y);
            let top_h = y.saturating_sub(layout.editor.y);
            let height = crate::common::view::splitter::clamp_split_px(top_h, track_h, 20, 20);
            Some(SqlTabMessage::SetEditorTopHeight { tab_id, height })
        }
        SqlSplitter::EditorHistory => {
            // When the History detail is visible the history zone fills the
            // body to its right edge (`history_zone_x` .. `area.right()`), so
            // the zone width is measured to `area.right()`; otherwise the plain
            // history pane's right edge. Measuring to the wrong edge would let
            // the stored width and the rendered zone disagree.
            let (instance, connection) = super::super::session::session_view_key(&tab.session);
            let detail_visible = tab.focus == super::super::state::SqlFocus::History
                && !state.history_store.entries(&instance, &connection).is_empty();
            let right_edge = if detail_visible {
                body.right()
            } else {
                layout.history.right()
            };
            let width = right_edge.saturating_sub(x);
            Some(SqlTabMessage::SetHistoryWidth { tab_id, width })
        }
        SqlSplitter::HistoryDetail => {
            let width = detail_width_for_x(body, &layout, tab.history.splitter.detail_pane_width, x);
            Some(SqlTabMessage::SetHistoryDetailWidth { tab_id, width })
        }
        SqlSplitter::ResultsDetail => {
            use crate::features::sql_workspace::sql_tab::results::splitter::view::{
                block_inner, detail_width_for_x,
            };
            let inner = block_inner(layout.results);
            let width = detail_width_for_x(inner, tab.results.splitter.detail_pane_width, x);
            Some(SqlTabMessage::SetResultsDetailWidth { tab_id, width })
        }
    }
}

/// Render the two SQL-tab splitter strips.
pub fn render(
    frame: &mut Frame,
    h_splitter: Rect,
    v_splitter: Rect,
    hover_editor_results: bool,
    dragging_editor_results: bool,
    hover_editor_history: bool,
    dragging_editor_history: bool,
) {
    draw(frame, h_splitter, SplitOrientation::Horizontal, hover_editor_results, dragging_editor_results);
    draw(frame, v_splitter, SplitOrientation::Vertical, hover_editor_history, dragging_editor_history);
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
        state.history_store
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
    fn splitter_at_with_detail_keeps_editor_results_hittable() {
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(body, 18, 24);
        let detail_w = 40u16;
        // When detail is visible, clicking on the horizontal splitter must
        // still resolve to EditorResults — the h_splitter row geometry is
        // unaffected by the detail panel expansion.
        let result = splitter_at_with_detail(&layout, body, layout.h_splitter.x, layout.h_splitter.y, true, detail_w);
        assert_eq!(result, Some(SqlSplitter::EditorResults));
        // The vertical editor/history splitter must also still be hittable.
        let zone_x = crate::features::sql_workspace::sql_tab::history::splitter::view::history_zone_x(body, &layout, detail_w);
        let a_x = zone_x.saturating_sub(1);
        let result_a = splitter_at_with_detail(&layout, body, a_x, layout.v_splitter.y, true, detail_w);
        assert_eq!(result_a, Some(SqlSplitter::EditorHistory));
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

    #[test]
    fn v_splitter_and_h_splitter_do_not_overlap() {
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(body, 18, 24);

        assert_eq!(layout.h_splitter.height, 1);
        assert_eq!(layout.v_splitter.width, 1);

        // The h_splitter row must be BELOW the v_splitter bottom.
        assert!(layout.h_splitter.y >= layout.v_splitter.bottom(),
            "h_splitter at row {} should be at or below v_splitter bottom at row {}",
            layout.h_splitter.y, layout.v_splitter.bottom());

        // Hovering over the v_splitter must return EditorHistory, NOT EditorResults.
        let vx = layout.v_splitter.x;
        let vy = layout.v_splitter.y;
        assert_eq!(splitter_at(&layout, vx, vy), Some(SqlSplitter::EditorHistory));
        assert_eq!(splitter_at(&layout, vx, vy + layout.v_splitter.height - 1), Some(SqlSplitter::EditorHistory));

        // Hovering over the h_splitter must return EditorResults.
        let hx = layout.h_splitter.x;
        let hy = layout.h_splitter.y;
        assert_eq!(splitter_at(&layout, hx, hy), Some(SqlSplitter::EditorResults));
    }

    #[test]
    fn v_splitter_hit_isolated_from_h_splitter_at_boundary() {
        // When detail is visible, the v_splitter shifts but must still not
        // overlap the h_splitter, and hit-testing must be unambiguous.
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(body, 18, 24);
        let detail_w = 40u16;

        // v_splitter hit at its top should NOT match h_splitter.
        let zone_x = crate::features::sql_workspace::sql_tab::history::splitter::view::history_zone_x(body, &layout, detail_w);
        let vx = zone_x.saturating_sub(1);
        let vy = layout.v_splitter.y;
        assert_eq!(
            splitter_at_with_detail(&layout, body, vx, vy, true, detail_w),
            Some(SqlSplitter::EditorHistory),
            "top of shifted v_splitter must be EditorHistory"
        );

        // h_splitter hit must still work.
        assert_eq!(
            splitter_at_with_detail(&layout, body, layout.h_splitter.x, layout.h_splitter.y, true, detail_w),
            Some(SqlSplitter::EditorResults),
            "h_splitter must still be EditorResults"
        );

        // At the boundary row (v_splitter.bottom() == h_splitter.y),
        // the hit must resolve to h_splitter, not v_splitter.
        if layout.h_splitter.y == layout.v_splitter.bottom() {
            assert_eq!(
                splitter_at_with_detail(&layout, body, vx, layout.v_splitter.bottom(), true, detail_w),
                Some(SqlSplitter::EditorResults),
                "at boundary row, h_splitter should win"
            );
        }
    }
}
