//! Geometry of the explorer instances/objects horizontal splitter.
//!
//! Single source of truth for where the instances tree (top), the splitter
//! row, and the objects tree (bottom) sit. Both the view (to render) and the
//! run loop (to hit-test mouse drags) call [`explorer_body_layout`].

use ratatui::Frame;
use ratatui::layout::Rect;

use super::layout::ExplorerSplitLayout;
use crate::common::layout::splitter::{clamp_split_px, hit};
use crate::common::view::splitter::{SplitOrientation, draw};

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

/// Render the explorer instances/objects horizontal splitter strip.
pub fn render(frame: &mut Frame, layout: &ExplorerSplitLayout, hover: bool, dragging: bool) {
    draw(
        frame,
        layout.splitter,
        SplitOrientation::Horizontal,
        hover,
        dragging,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::explorer::splitter::layout::explorer_body_layout;

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
