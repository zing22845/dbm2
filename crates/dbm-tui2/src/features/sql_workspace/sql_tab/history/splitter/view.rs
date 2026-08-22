//! Geometry of the History-internal detail/list splitter (B).
//!
//! When the detail preview is visible, the History zone widens leftward to
//! hold both the detail (left) and the list (right), separated by splitter B.
//! The zone width = list + detail + splitter (mirrors the original dbm's
//! `history_zone_width`). Dragging B re-allocates detail vs list.

use ratatui::layout::Rect;

use super::state::clamp_detail_pane_width;
use crate::features::sql_workspace::sql_tab::layout::SqlTabLayout;
use crate::features::sql_workspace::sql_tab::splitter::state::MIN_SQL_PANE_WIDTH;

/// The width of the History zone when the detail is visible: the list width
/// plus the *actual* detail pane width plus a splitter. Dragging B re-allocates
/// detail vs list within the zone; the zone itself is set by the editor/history
/// splitter (A).
pub fn history_zone_width(layout: &SqlTabLayout, detail_pane_width: u16) -> u16 {
    let detail_w = clamp_detail_pane_width(detail_pane_width);
    layout.history.width + detail_w + 1
}

/// The left edge of the widened History zone (it extends left of the base
/// history pane, eating into the editor), clamped so the editor always keeps a
/// minimum width. The zone can never fill more than `area.width - MIN_SQL_PANE_WIDTH`,
/// otherwise a very wide history pane would squeeze the editor to zero.
pub fn history_zone_x(area: Rect, layout: &SqlTabLayout, detail_pane_width: u16) -> u16 {
    let max_zone_w = area.width.saturating_sub(MIN_SQL_PANE_WIDTH).max(1);
    let zone_w = history_zone_width(layout, detail_pane_width).min(max_zone_w);
    area.x
        .max(layout.history.right().saturating_sub(zone_w))
        .min(area.right().saturating_sub(MIN_SQL_PANE_WIDTH))
}

/// The rect of splitter B (the detail/list boundary), if the detail is visible.
pub fn history_detail_splitter(
    area: Rect,
    layout: &SqlTabLayout,
    detail_visible: bool,
    detail_pane_width: u16,
) -> Option<Rect> {
    if !detail_visible {
        return None;
    }
    let detail_w = clamp_detail_pane_width(detail_pane_width);
    let zone_x = history_zone_x(area, layout, detail_pane_width);
    // The splitter sits at the right edge of the detail, inside the History
    // border (the border is 1 col wide, so the splitter is at
    // zone_x + 1 + detail_w), matching where the renderer draws it.
    Some(Rect::new(zone_x + 1 + detail_w, layout.history.y, 1, layout.history.height))
}

/// Compute the detail pane width for a drag of splitter B at `x`: the distance
/// from the History zone's content left edge to the pointer.
pub fn detail_width_for_x(area: Rect, layout: &SqlTabLayout, detail_pane_width: u16, x: u16) -> u16 {
    let zone_x = history_zone_x(area, layout, detail_pane_width);
    x.saturating_sub(zone_x).saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sql_workspace::sql_tab::layout::sql_tab_layout;

    #[test]
    fn zone_width_and_splitter_move_with_detail() {
        let area = Rect::new(0, 0, 120, 40);
        let layout = sql_tab_layout(area, 45, 24);
        // zone = list(24) + detail(40) + splitter.
        assert_eq!(history_zone_width(&layout, 40), 24 + 40 + 1);
        // A wider detail widens the zone (list stays fixed).
        assert!(history_zone_width(&layout, 56) > history_zone_width(&layout, 40));
        // No detail -> no internal splitter.
        assert!(history_detail_splitter(area, &layout, false, 40).is_none());
    }

    #[test]
    fn detail_splitter_hit_resolves_correctly() {
        let area = Rect::new(0, 0, 120, 40);
        let layout = sql_tab_layout(area, 45, 24);
        let detail_split = history_detail_splitter(area, &layout, true, 40).unwrap();
        let zone_x = history_zone_x(area, &layout, 40);
        assert_eq!(detail_split.x, zone_x + 1 + 40);
    }
}
