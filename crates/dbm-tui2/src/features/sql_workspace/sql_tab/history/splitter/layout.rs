//! History zone geometry: where the history/detail splitter sits.

use super::state::clamp_detail_pane_width;
use crate::features::sql_workspace::sql_tab::layout::SqlTabLayout;
use crate::features::sql_workspace::sql_tab::splitter::state::MIN_SQL_PANE_WIDTH;
use ratatui::layout::Rect;

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

/// The width of the History zone when the detail is visible: the list width
/// plus the *actual* detail pane width plus the internal splitter, plus the
/// History border that wraps the whole zone. Dragging B re-allocates detail vs
/// list within the zone; the zone itself is set by the editor/history splitter
/// (A).
///
/// The stored `layout.history.width` is the list pane's *outer* width (it
/// carries the History border, exactly like the standalone history pane), so a
/// visible detail adds `detail + splitter` content columns and the border adds
/// 2. Including the border here keeps the rendered list width
/// (`zone_w - 2 - detail - 1`) equal to the stored list width.
pub fn history_zone_width(layout: &SqlTabLayout, detail_pane_width: u16) -> u16 {
    let detail_w = clamp_detail_pane_width(detail_pane_width);
    layout.history.width + detail_w + 1 + 2
}
