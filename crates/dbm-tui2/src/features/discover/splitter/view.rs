//! Geometry of the discover targets/results horizontal splitter.
//!
//! Single source of truth for where the targets editor (top), the splitter
//! row, and the results list (bottom) sit. Both the view (to render) and the
//! run loop (to hit-test mouse drags) call [`discover_body_layout`].

use ratatui::Frame;
use ratatui::layout::Rect;

use super::layout::DiscoverSplitLayout;
use crate::common::layout::splitter::{clamp_split_px, hit};
use crate::common::view::splitter::{SplitOrientation, draw};

/// The splitter row the position `(x, y)` is on, if any.
pub fn splitter_at(layout: &DiscoverSplitLayout, x: u16, y: u16) -> bool {
    hit(layout.splitter, x, y)
}

/// Compute the targets height (rows) for a drag of the splitter at `y`:
/// the distance from the area's top to the pointer, clamped to the valid
/// `[20%, 80%]` row range of the track.
pub fn targets_height_for_y(area: Rect, y: u16) -> u16 {
    let track_h = area.height;
    let top_h = y.saturating_sub(area.y);
    clamp_split_px(top_h, track_h, 20, 20)
}

/// The targets/results track height (rows) for a given area, used when
/// materializing a persisted percentage back to rows.
pub fn track_height(area: Rect) -> u16 {
    area.height
}

/// Render the discover targets/results horizontal splitter strip.
pub fn render(frame: &mut Frame, layout: &DiscoverSplitLayout, hover: bool, dragging: bool) {
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
    use crate::features::discover::splitter::layout::discover_body_layout;

    #[test]
    fn layout_places_targets_splitter_results() {
        let area = Rect::new(0, 0, 80, 30);
        let layout = discover_body_layout(area, 10);
        assert_eq!(layout.targets.height, 10);
        assert_eq!(layout.splitter.height, 1);
        assert_eq!(layout.splitter.y, 10);
        assert_eq!(layout.results.y, 11);
        assert_eq!(layout.targets.x, layout.results.x);
    }

    #[test]
    fn layout_clamps_targets_height() {
        let area = Rect::new(0, 0, 80, 30);
        // A huge requested height keeps results above 20%.
        let layout = discover_body_layout(area, 100);
        assert!(layout.results.height >= 5);
        // A tiny track returns empty.
        assert_eq!(
            discover_body_layout(Rect::new(0, 0, 80, 2), 10),
            DiscoverSplitLayout::default()
        );
    }

    #[test]
    fn hit_detects_splitter_row() {
        let area = Rect::new(0, 0, 80, 30);
        let layout = discover_body_layout(area, 10);
        assert!(splitter_at(&layout, 5, layout.splitter.y));
        assert!(!splitter_at(&layout, 5, 5));
    }

    #[test]
    fn height_from_y_clamps_to_valid_range() {
        let area = Rect::new(0, 0, 80, 30);
        assert_eq!(targets_height_for_y(area, 15), 15);
        assert_eq!(targets_height_for_y(area, 0), area.height / 5);
        assert_eq!(targets_height_for_y(area, 1000), 30 - 1 - 30 / 5);
    }
}
