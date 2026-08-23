//! Geometry of the explorer instances/objects horizontal splitter.
//!
//! Single source of truth for where the instances tree (top), the splitter
//! row, and the objects tree (bottom) sit. Both the view (to render) and the
//! run loop (to hit-test mouse drags) call [`explorer_body_layout`].

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::common::view::splitter::clamp_split_px;
use crate::common::view::splitter::hit;

/// The panes computed by [`explorer_body_layout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExplorerSplitLayout {
    /// The instances tree (top).
    pub instances: Rect,
    /// The 1-row horizontal splitter between instances and objects.
    pub splitter: Rect,
    /// The objects tree (bottom).
    pub objects: Rect,
    /// Actual instances-height bounds (rows), what the layout clamped to.
    /// Nudge/drag clamp to these so the stored value and the rendered split
    /// always agree (no redundant repaint at the boundary).
    pub instances_min: u16,
    pub instances_max: u16,
}

/// Compute the explorer instances/objects layout. `instances_height` is the
/// stored top-pane height in rows, clamped to `[20%, 80%]` of the current track
/// (so a height recorded on a taller terminal is re-clamped correctly after a
/// resize). Returns an empty layout when the track is too small.
pub fn explorer_body_layout(area: Rect, instances_height: u16) -> ExplorerSplitLayout {
    let empty = ExplorerSplitLayout::default();
    if area.height < 3 {
        return empty;
    }
    // Recompute the track from the current area so the stored rows are clamped
    // against the *live* height (handles terminal resizes).
    let track_h = area.height;
    let instances_min = (track_h * 20) / 100;
    let instances_max = track_h.saturating_sub(1).saturating_sub(instances_min);
    let top_px = instances_height.clamp(instances_min, instances_max);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_px),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);
    if rows.len() != 3 || rows[2].height < 1 {
        return empty;
    }
    ExplorerSplitLayout {
        instances: rows[0],
        splitter: rows[1],
        objects: rows[2],
        instances_min,
        instances_max,
    }
}

/// The splitter row the position `(x, y)` is on, if any.
pub fn splitter_at(layout: &ExplorerSplitLayout, x: u16, y: u16) -> bool {
    hit(layout.splitter, x, y)
}

/// Compute the instances height (rows) for a drag of the splitter at `y`: the
/// distance from the area's top to the pointer, clamped to the valid
/// `[20%, 80%]` row range of the track.
pub fn instances_height_for_y(area: Rect, y: u16) -> u16 {
    let track_h = area.height;
    let top_h = y.saturating_sub(area.y);
    clamp_split_px(top_h, track_h, 20, 20)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_places_instances_splitter_objects() {
        let area = Rect::new(0, 3, 40, 21);
        let layout = explorer_body_layout(area, 10);
        assert_eq!(layout.instances.height, 10);
        assert_eq!(layout.splitter.height, 1);
        assert_eq!(layout.splitter.y, 13);
        assert_eq!(layout.objects.y, 14);
        assert_eq!(layout.instances.x, layout.objects.x);
    }

    #[test]
    fn layout_clamps_instances_height() {
        let area = Rect::new(0, 3, 40, 21);
        let layout = explorer_body_layout(area, 100);
        assert!(layout.objects.height >= 4);
        assert_eq!(
            explorer_body_layout(Rect::new(0, 3, 40, 2), 10),
            ExplorerSplitLayout::default()
        );
    }

    #[test]
    fn hit_detects_splitter_row() {
        let area = Rect::new(0, 3, 40, 21);
        let layout = explorer_body_layout(area, 10);
        assert!(splitter_at(&layout, 5, layout.splitter.y));
        assert!(!splitter_at(&layout, 5, 5));
    }

    #[test]
    fn height_from_y_clamps_to_valid_range() {
        let area = Rect::new(0, 3, 40, 21);
        assert_eq!(instances_height_for_y(area, 13), 10);
        assert_eq!(instances_height_for_y(area, 1000), 21 - 1 - 21 / 5);
    }
}
