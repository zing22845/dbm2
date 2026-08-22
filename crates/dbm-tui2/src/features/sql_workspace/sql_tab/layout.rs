//! Pure layout for the SQL tab body.
//!
//! The single source of truth for where each pane and splitter sits. Both the
//! view (to render) and the run loop (to hit-test mouse drags) call
//! [`sql_tab_layout`], so a splitter can only ever be found where it is drawn.
//!
//! Mirrors the original dbm `sql_tab_layout` (ui.rs §11): a vertical split puts
//! the editor+history row on top and results on the bottom; the top row is a
//! horizontal split with the SQL editor on the left and the history on the
//! right. The split positions come from the tab's stored ratio/width, clamped
//! to the current track so neither pane can collapse.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::common::view::splitter::{clamp_split_px, hit};

/// Identifies which splitter a mouse position is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlSplitter {
    /// Horizontal splitter: editor+history row vs results.
    EditorResults,
    /// Vertical splitter: editor vs history in the top row.
    EditorHistory,
    /// Vertical splitter inside the History pane: the detail preview vs the
    /// history list. Dragging it re-allocates width between detail and list
    /// while the total History zone width stays constant.
    HistoryDetail,
}

/// Minimum width of the SQL editor pane, so the detail zone can never push the
/// editor to zero (mirrors the original dbm's `MIN_SQL_PANE_WIDTH`).
pub const MIN_SQL_PANE_WIDTH: u16 = 20;

/// Compute the rect of the History pane's internal detail/list splitter, if
/// the detail is currently visible. This depends on the tab's focus, detail
/// width, and the widened history zone, so it cannot be part of the static
/// [`sql_tab_layout`] — the renderer and the run loop compute it identically.
/// The fixed width of the History zone when the detail is visible: the base
/// history (list) width plus the default detail width plus a splitter. This is
/// held constant while dragging the internal detail/list splitter, which only
/// re-allocates width between the detail and the list.
pub fn history_zone_width(layout: &SqlTabLayout) -> u16 {
    use crate::features::sql_workspace::sql_tab::history::detail::DEFAULT_DETAIL_PANE_WIDTH;
    layout.history.width + DEFAULT_DETAIL_PANE_WIDTH + 1
}

/// The left edge of the widened History zone (it extends left of the base
/// history pane, eating into the editor), clamped so the editor always keeps a
/// minimum width. The zone can never fill more than `area.width - MIN_SQL_PANE_WIDTH`,
/// otherwise a very wide history pane (set by dragging the editor/history
/// splitter) would squeeze the editor to zero and hang edtui's wrapped render.
pub fn history_zone_x(area: Rect, layout: &SqlTabLayout) -> u16 {
    let max_zone_w = area.width.saturating_sub(MIN_SQL_PANE_WIDTH).max(1);
    let zone_w = history_zone_width(layout).min(max_zone_w);
    area.x
        .max(layout.history.right().saturating_sub(zone_w))
        .min(area.right().saturating_sub(MIN_SQL_PANE_WIDTH))
}

/// Compute the rect of the History pane's internal detail/list splitter, if
/// the detail is currently visible. The History zone width is fixed; dragging
/// this splitter re-allocates detail vs list within that fixed zone.
pub fn history_detail_splitter(
    area: Rect,
    layout: &SqlTabLayout,
    detail_visible: bool,
    detail_pane_width: u16,
) -> Option<Rect> {
    if !detail_visible {
        return None;
    }
    let detail_w = crate::features::sql_workspace::sql_tab::history::detail::clamp_detail_pane_width(
        detail_pane_width,
    );
    let zone_x = history_zone_x(area, layout);
    // The splitter sits at the right edge of the detail, inside the History
    // border (the border is 1 col wide, so the splitter is at
    // zone_x + 1 + detail_w), matching where the renderer draws it.
    Some(Rect::new(zone_x + 1 + detail_w, layout.history.y, 1, layout.history.height))
}

/// The panes and splitter strips computed by [`sql_tab_layout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SqlTabLayout {
    pub editor: Rect,
    pub history: Rect,
    pub results: Rect,
    /// The 1-row horizontal splitter between the top row and results.
    pub h_splitter: Rect,
    /// The 1-column vertical splitter between editor and history.
    pub v_splitter: Rect,
}

/// Compute the SQL tab body layout. `area` is the region below the tab bar.
pub fn sql_tab_layout(area: Rect, split_ratio: u8, history_width: u16) -> SqlTabLayout {
    let empty = SqlTabLayout::default();

    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0), // top row (editor + history)
            Constraint::Length(1), // horizontal splitter
            Constraint::Min(0), // results
        ])
        .split(area);
    if body.len() != 3 || body[0].height < 2 || body[2].height < 2 {
        return empty;
    }

    // Editor top-pane height: split_ratio% of the body, clamped to [20%, 80%]
    // of the track so neither the top row nor results collapses.
    let track_h = body[0].height + 1 + body[2].height;
    let top_px = ((u32::from(track_h) * u32::from(split_ratio)) / 100) as u16;
    let row_h = clamp_split_px(top_px, track_h, 20, 20);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(row_h),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);
    if vertical.len() != 3 {
        return empty;
    }
    let (top_row, h_splitter, results) = (vertical[0], vertical[1], vertical[2]);
    if top_row.height < 2 {
        return empty;
    }

    // Top row horizontal split: editor (left) + vertical splitter + history.
    let track_w = top_row.width;
    let history_w = history_width
        .clamp(12, track_w.saturating_sub(30).max(12));
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(history_w),
        ])
        .split(top_row);
    if top.len() != 3 || top[0].width < 1 {
        return empty;
    }
    let (editor, v_splitter, history) = (top[0], top[1], top[2]);

    SqlTabLayout {
        editor,
        history,
        results,
        h_splitter,
        v_splitter,
    }
}

impl SqlTabLayout {
    /// The splitter the position `(x, y)` is on, if any.
    pub fn splitter_at(&self, x: u16, y: u16) -> Option<SqlSplitter> {
        if hit(self.h_splitter, x, y) {
            return Some(SqlSplitter::EditorResults);
        }
        if hit(self.v_splitter, x, y) {
            return Some(SqlSplitter::EditorHistory);
        }
        None
    }

    /// Like [`splitter_at`](Self::splitter_at), but when the History detail is
    /// visible the editor is shrunk and the history zone is widened, so both the
    /// editor/history splitter and the internal detail/list splitter move. Hit
    /// test against those relocated positions instead of the stale base
    /// `v_splitter`. `area` is the full SQL-tab body region.
    pub fn splitter_at_with_detail(
        &self,
        area: Rect,
        x: u16,
        y: u16,
        detail_visible: bool,
        detail_pane_width: u16,
    ) -> Option<SqlSplitter> {
        if !detail_visible {
            return self.splitter_at(x, y);
        }
        // The editor/history splitter moves to the widened zone's left edge
        // (the editor is shrunk); the internal detail/list splitter sits just
        // right of the detail.
        let zone_x = history_zone_x(area, self);
        let editor_history = Rect::new(zone_x.saturating_sub(1), self.v_splitter.y, 1, self.v_splitter.height);
        if hit(editor_history, x, y) {
            return Some(SqlSplitter::EditorHistory);
        }
        if let Some(r) = history_detail_splitter(area, self, true, detail_pane_width) {
            if hit(r, x, y) {
                return Some(SqlSplitter::HistoryDetail);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_places_all_panes() {
        let area = Rect::new(0, 0, 120, 40);
        let layout = sql_tab_layout(area, 45, 24);
        assert!(layout.editor.width > 0 && layout.editor.height > 0);
        assert!(layout.history.width > 0 && layout.history.height > 0);
        assert!(layout.results.height > 0);
        // Editor left of history, both in the top row above results.
        assert!(layout.editor.x < layout.history.x);
        assert!(layout.editor.y == layout.history.y);
        assert!(layout.editor.y + layout.editor.height <= layout.results.y);
        // Splitter rows have width/height 1.
        assert_eq!(layout.h_splitter.height, 1);
        assert_eq!(layout.v_splitter.width, 1);
    }

    #[test]
    fn splitter_hit_test_identifies_both() {
        let area = Rect::new(0, 0, 120, 40);
        let layout = sql_tab_layout(area, 45, 24);
        assert_eq!(
            layout.splitter_at(layout.v_splitter.x, layout.v_splitter.y),
            Some(SqlSplitter::EditorHistory)
        );
        assert_eq!(
            layout.splitter_at(layout.h_splitter.x, layout.h_splitter.y),
            Some(SqlSplitter::EditorResults)
        );
        assert_eq!(layout.splitter_at(1, 1), None);
    }

    #[test]
    fn layout_tiny_area_returns_empty() {
        assert_eq!(
            sql_tab_layout(Rect::new(0, 0, 5, 2), 45, 24),
            SqlTabLayout::default()
        );
    }

    #[test]
    fn history_detail_splitter_keeps_zone_constant() {
        // The internal detail/list splitter re-allocates width while the total
        // History zone width stays fixed.
        let area = Rect::new(0, 0, 120, 40);
        let layout = sql_tab_layout(area, 45, 24);
        let zone_w = history_zone_width(&layout);
        assert_eq!(zone_w, 24 + 40 + 1); // base history + default detail + splitter

        // With the default detail width (40), the splitter sits at the detail's
        // right edge inside the border.
        let s1 = history_detail_splitter(area, &layout, true, 40).unwrap();
        // A wider detail moves the splitter right (list shrinks); the zone's
        // left edge is unchanged, so the total zone width stays constant.
        let s2 = history_detail_splitter(area, &layout, true, 56).unwrap();
        assert!(s2.x > s1.x, "wider detail must push the splitter right");
        assert_eq!(history_zone_x(area, &layout), layout.history.x.saturating_sub(zone_w - layout.history.width));

        // No detail -> no internal splitter.
        assert!(history_detail_splitter(area, &layout, false, 40).is_none());
    }

    #[test]
    fn detail_splitter_hit_test_resolves_correctly() {
        let area = Rect::new(0, 0, 120, 40);
        let layout = sql_tab_layout(area, 45, 24);

        // With the detail visible, a click on the internal detail/list splitter
        // must resolve to HistoryDetail (not the stale base editor/history
        // splitter), and a click on the relocated editor/history splitter to
        // EditorHistory.
        let zone_x = history_zone_x(area, &layout);
        let detail_split = history_detail_splitter(area, &layout, true, 40).unwrap();
        assert_eq!(
            layout.splitter_at_with_detail(area, detail_split.x, detail_split.y, true, 40),
            Some(SqlSplitter::HistoryDetail),
            "a click on the internal splitter must be HistoryDetail"
        );
        assert_eq!(
            layout.splitter_at_with_detail(
                area,
                zone_x.saturating_sub(1),
                layout.v_splitter.y + 1,
                true,
                40
            ),
            Some(SqlSplitter::EditorHistory),
            "a click on the relocated editor/history splitter must be EditorHistory"
        );

        // Without the detail, the internal splitter is not present.
        assert_eq!(
            layout.splitter_at_with_detail(area, detail_split.x, detail_split.y, false, 40),
            None
        );
    }
}
