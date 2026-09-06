//! Splitter size messages: the editor/history/results panes and their
//! detail panes (absolute widths and nudges).

use super::super::msg::SqlTabMessage;
use super::super::state::SqlTabState;
use super::{SqlTabOut, session_key, warn_tab_missing};

pub(super) fn apply(msg: SqlTabMessage, state: &mut SqlTabState, out: &mut SqlTabOut) {
    match msg {
        SqlTabMessage::SetEditorTopHeight { tab_id, height } => {
            if let Some(idx) = state.index_of(tab_id) {
                // Only repaint when the split actually moved — a drag that does
                // not change the split (e.g. at a clamp boundary, or the pointer
                // resting on a row it already set) must not count as a redundant
                // redraw and inflate the waste metric.
                out.dirty |= state.tabs[idx].set_editor_top_height(height);
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::NudgeEditorTopHeight { tab_id, plus } => {
            if let Some(idx) = state.index_of(tab_id) {
                // `+` grows the focused pane: the top row (editor/history) or
                // the bottom row (results), per the tab's current focus.
                let top_focused = matches!(
                    state.tabs[idx].focus,
                    crate::features::sql_workspace::sql_tab::state::SqlFocus::Editor
                        | crate::features::sql_workspace::sql_tab::state::SqlFocus::History
                );
                out.dirty |= state.tabs[idx].nudge_editor_top_height(plus, top_focused);
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::SetHistoryWidth { tab_id, width } => {
            // `width` is the total History zone width (from splitter A to the
            // right edge). When the detail is visible the zone = list + detail +
            // splitter, and dragging A must keep the detail width fixed and
            // change the list + editor (original dbm behavior 1). So the stored
            // list width is `zone - detail - splitter`; without the detail the
            // list IS the zone.
            if let Some(idx) = state.index_of(tab_id) {
                let tab = &state.tabs[idx];
                let (instance, connection) = session_key(&tab.session);
                let detail_visible = super::history::detail_visible(
                    tab.focus == crate::features::sql_workspace::sql_tab::state::SqlFocus::History,
                    &tab.history.list,
                    &state.history_store,
                    &instance,
                    &connection,
                );
                let list_w = if detail_visible {
                    // `width` is the whole zone (A to the right edge), which
                    // holds list + detail + splitter + the History border. The
                    // stored list width is the list pane's *outer* width (it
                    // carries the border), so subtract detail + splitter + 2.
                    width
                        .saturating_sub(tab.history.splitter.detail_pane_width)
                        .saturating_sub(1) // splitter
                        .saturating_sub(2) // History border (left + right)
                } else {
                    width
                };
                // Clamp to the layout's actual history bounds intersected with
                // the storage range [MIN_HISTORY_WIDTH, MAX_HISTORY_WIDTH]
                // `set_history_pane_width` clamps to, so dragging past the
                // boundary leaves the stored width unchanged (no redundant
                // repaint) and never disagrees with the setter.
                use crate::features::sql_workspace::sql_tab::splitter::state::{
                    MAX_HISTORY_WIDTH, MIN_HISTORY_WIDTH,
                };
                let lo = tab.splitter.history_min.max(MIN_HISTORY_WIDTH);
                let mut hi = tab.splitter.history_max.min(MAX_HISTORY_WIDTH);
                // When the detail is visible the stored width is the *list*
                // width (`zone - detail - splitter`). The zone's widest reach is
                // `area.width - MIN_SQL_PANE_WIDTH` (the editor keeps its min
                // width), so the list must stop at that minus the detail pane
                // and the splitter — otherwise `history_zone_x` clamps the zone
                // to a narrower maximum and the stored width disagrees with the
                // rendered geometry (redundant repaints at the drag limit).
                if detail_visible {
                    // The list pane carries the History border, so its upper
                    // bound is `history_max` (the no-detail list max) minus the
                    // detail pane, splitter and the border it would otherwise
                    // own.
                    hi = hi
                        .saturating_sub(tab.history.splitter.detail_pane_width)
                        .saturating_sub(2); // History border
                }
                let clamped = list_w.clamp(lo, hi);
                let changed = tab.splitter.history_pane_width != clamped;
                state.tabs[idx].set_history_pane_width(clamped);
                out.dirty = changed;
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::SetHistoryStore { tab_id: _, store } => {
            state.history_store = store;
            out.dirty = true;
        }
        SqlTabMessage::SetHistoryDetailWidth { tab_id, width } => {
            if let Some(idx) = state.index_of(tab_id) {
                let before = state.tabs[idx].history.splitter.detail_pane_width;
                state.tabs[idx]
                    .history
                    .splitter
                    .set_detail_pane_width(width);
                out.dirty = before != state.tabs[idx].history.splitter.detail_pane_width;
                // The History zone holds list + detail + splitter, capped at
                // `area.width - MIN_SQL_PANE_WIDTH` (the editor keeps its min
                // width). Growing the detail past that boundary would squeeze
                // the *rendered* list below its stored width (`history_zone_x`
                // clamps the zone), so list and detail would disagree and the
                // next drag would emit redundant repaints. Keep the list within
                // `history_max - detail` — exactly the editor-min boundary.
                use crate::features::sql_workspace::sql_tab::splitter::state::{
                    MAX_HISTORY_WIDTH, MIN_HISTORY_WIDTH,
                };
                let (instance, connection) = session_key(&state.tabs[idx].session);
                let detail_visible = super::history::detail_visible(
                    state.tabs[idx].focus
                        == crate::features::sql_workspace::sql_tab::state::SqlFocus::History,
                    &state.tabs[idx].history.list,
                    &state.history_store,
                    &instance,
                    &connection,
                );
                if detail_visible {
                    let tab = &state.tabs[idx];
                    let hi = tab
                        .splitter
                        .history_max
                        .min(MAX_HISTORY_WIDTH)
                        .saturating_sub(tab.history.splitter.detail_pane_width)
                        .saturating_sub(2); // History border
                    let lo = tab.splitter.history_min.max(MIN_HISTORY_WIDTH);
                    if tab.splitter.history_pane_width > hi {
                        let clamped = tab.splitter.history_pane_width.clamp(lo, hi);
                        state.tabs[idx].splitter.history_pane_width = clamped;
                        out.dirty = true;
                    }
                }
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::NudgeHistoryDetailWidth { tab_id, nudge } => {
            if let Some(idx) = state.index_of(tab_id) {
                let delta = crate::common::layout::splitter::width_delta_for_left_pane(
                    nudge,
                    crate::common::layout::splitter::WIDTH_NUDGE_STEP,
                );
                let next = (state.tabs[idx].history.splitter.detail_pane_width as i16 + delta)
                    .max(0) as u16;
                let before = state.tabs[idx].history.splitter.detail_pane_width;
                state.tabs[idx].history.splitter.set_detail_pane_width(next);
                out.dirty = before != state.tabs[idx].history.splitter.detail_pane_width;
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::SetResultsDetailWidth { tab_id, width } => {
            if let Some(idx) = state.index_of(tab_id) {
                let before = state.tabs[idx].results.splitter.detail_pane_width;
                state.tabs[idx]
                    .results
                    .splitter
                    .set_detail_pane_width(width);
                out.dirty = before != state.tabs[idx].results.splitter.detail_pane_width;
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::NudgeResultsDetailWidth { tab_id, nudge } => {
            if let Some(idx) = state.index_of(tab_id) {
                // Results detail is on the RIGHT side of its splitter, so the
                // delta flips sign vs. History detail (which is on the left).
                let delta = crate::common::layout::splitter::width_delta_for_right_pane(
                    nudge,
                    crate::common::layout::splitter::WIDTH_NUDGE_STEP,
                );
                let next = (state.tabs[idx].results.splitter.detail_pane_width as i16 + delta)
                    .max(0) as u16;
                let before = state.tabs[idx].results.splitter.detail_pane_width;
                state.tabs[idx].results.splitter.set_detail_pane_width(next);
                out.dirty = before != state.tabs[idx].results.splitter.detail_pane_width;
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::NudgeHistoryWidth { tab_id, nudge } => {
            if let Some(idx) = state.index_of(tab_id) {
                // The width is clamped to the live editor+history track so the
                // editor keeps its minimum width; nudging past the boundary
                // leaves the stored width unchanged (no redundant repaint).
                out.dirty |= state.tabs[idx].splitter.nudge_history_width(nudge);
            } else {
                warn_tab_missing(tab_id);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sql_workspace::sql_tab::update::update;

    #[test]
    fn set_history_width_with_detail_visible_changes_list_not_detail() {
        // Behavior 1: dragging splitter A (editor/history) keeps the detail
        // width fixed and changes the list + editor. The message's `width` is
        // the whole zone (from A to the right edge); the stored list width must
        // become `zone - detail - splitter`.
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;
        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        s.tabs[0].focus = SqlFocus::History;
        s.tabs[0].history.splitter.detail_pane_width = 40;
        s.tabs[0].splitter.history_pane_width = 24;
        s.tabs[0].splitter.history_max = 200; // so the drag width isn't clamped
        s.history_store.record_success("inst", "c1", "SELECT 1");

        // zone = 100, detail = 40, splitter = 1, border = 2
        // -> list = 100 - 40 - 1 - 2 = 57.
        let (s, _i, _e, _d) = update(SqlTabMessage::SetHistoryWidth { tab_id, width: 100 }, s);
        assert_eq!(
            s.tabs[0].splitter.history_pane_width, 57,
            "with the detail visible, A drags change the list, not the detail"
        );
        assert_eq!(
            s.tabs[0].history.splitter.detail_pane_width, 40,
            "the detail width must not change when dragging splitter A"
        );
    }
    #[test]
    fn set_history_width_with_detail_clamps_list_at_the_editor_min() {
        // When the detail is visible the list may only grow until the zone
        // reaches `area.width - MIN_SQL_PANE_WIDTH` (the editor keeps its min
        // width). Dragging splitter A past that must clamp the list to
        // `history_max - detail` and stop dirtying, so the stored width never
        // disagrees with the rendered zone.
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;
        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        s.tabs[0].focus = SqlFocus::History;
        s.tabs[0].history.splitter.detail_pane_width = 40;
        // Simulate the layout's `history_max` for a 120-wide body:
        // track - MIN_SQL_PANE_WIDTH - 1 = 120 - 20 - 1 = 99.
        s.tabs[0].splitter.history_max = 99;
        s.history_store.record_success("inst", "c1", "SELECT 1");

        // A drag far past the zone limit: the zone max is 120 - 20 = 100, so
        // the list can be at most 100 - 40 - 1 - 2 (border) = 57.
        let (s, _i, _e, d) = update(SqlTabMessage::SetHistoryWidth { tab_id, width: 150 }, s);
        assert_eq!(
            s.tabs[0].splitter.history_pane_width, 57,
            "the list must clamp at history_max - detail - border (zone max - detail - splitter - border)"
        );
        assert!(
            d,
            "the width changed from its default, so this run is dirty"
        );

        // Re-dragging to the same extreme must not dirty (no redundant repaint).
        let (s2, _i, _e, d2) = update(SqlTabMessage::SetHistoryWidth { tab_id, width: 150 }, s);
        assert_eq!(s2.tabs[0].splitter.history_pane_width, 57);
        assert!(
            !d2,
            "dragging past the limit must not keep dirtying (would waste repaints)"
        );
    }
    #[test]
    fn growing_detail_reclamps_list_so_the_zone_stays_consistent() {
        // With A (editor/history) already dragged to the limit, the list is
        // `history_max - detail`. Growing the detail past the boundary (B drag)
        // must re-clamp the list to `history_max - new_detail`, otherwise the
        // rendered list is squeezed below its stored width and the next drag
        // emits redundant repaints.
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;
        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        s.tabs[0].focus = SqlFocus::History;
        s.tabs[0].history.splitter.detail_pane_width = 40;
        s.tabs[0].splitter.history_max = 99; // 120-wide body: 120 - 20 - 1
        s.tabs[0].splitter.history_pane_width = 57; // A already at the limit (99 - 40 - 2 border)
        s.history_store.record_success("inst", "c1", "SELECT 1");

        // Drag B to grow the detail to its max (72).
        let (s, _i, _e, d) = update(
            SqlTabMessage::SetHistoryDetailWidth { tab_id, width: 200 },
            s,
        );
        assert_eq!(s.tabs[0].history.splitter.detail_pane_width, 72);
        assert_eq!(
            s.tabs[0].splitter.history_pane_width, 25,
            "the list must shrink to history_max - detail - border = 99 - 72 - 2"
        );
        assert!(
            d,
            "both the detail and the list changed, so this run is dirty"
        );

        // Repeating the same drag must not dirty (no redundant repaint).
        let (s2, _i, _e, d2) = update(
            SqlTabMessage::SetHistoryDetailWidth { tab_id, width: 200 },
            s,
        );
        assert_eq!(s2.tabs[0].history.splitter.detail_pane_width, 72);
        assert_eq!(s2.tabs[0].splitter.history_pane_width, 25);
        assert!(
            !d2,
            "repeating the same extreme drag must not dirty (would waste repaints)"
        );
    }
    #[test]
    fn set_history_width_without_detail_sets_list_directly() {
        // When the detail is hidden, the zone is just the list, so A drags set
        // the list width directly.
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;
        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        s.tabs[0].focus = SqlFocus::Editor; // no detail visible
        s.tabs[0].splitter.history_max = 200; // so the drag width isn't clamped

        let (s, _i, _e, _d) = update(SqlTabMessage::SetHistoryWidth { tab_id, width: 80 }, s);
        assert_eq!(s.tabs[0].splitter.history_pane_width, 80);
    }
}
