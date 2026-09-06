//! Hit-testing, layout, and resize math for the app's Explorer / workspace
//! vertical splitter. This is the single source of truth for where the two
//! panes and the splitter sit: both the view (to render) and the run loop (to
//! hit-test mouse drags) call [`app_body_layout`], so a splitter can only ever
//! be found where it is drawn.

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::common::layout::splitter::hit;
use crate::common::view::splitter::{SplitOrientation, draw};

use super::layout::AppBodyLayout;
use super::state::MIN_EXPLORER_WIDTH;

/// The splitter the position `(x, y)` is on, if any.
pub fn splitter_at(layout: &AppBodyLayout, x: u16, y: u16) -> bool {
    hit(layout.v_splitter, x, y)
}

/// Compute the explorer pane width for a drag of the splitter at `x`: the
/// distance from the area's left edge to the pointer, clamped to the allowed
/// range.
pub fn explorer_width_for_x(area: Rect, x: u16) -> u16 {
    let max_explorer = area
        .width
        .saturating_sub(1)
        .saturating_sub(4)
        .max(MIN_EXPLORER_WIDTH);
    x.saturating_sub(area.x)
        .clamp(MIN_EXPLORER_WIDTH, max_explorer)
}

/// Render the Explorer/workspace vertical splitter strip.
pub fn render(frame: &mut Frame, layout: &AppBodyLayout, hover: bool, dragging: bool) {
    draw(
        frame,
        layout.v_splitter,
        SplitOrientation::Vertical,
        hover,
        dragging,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::app_splitter::layout::app_body_layout;

    #[test]
    fn layout_places_explorer_splitter_workspace() {
        let area = Rect::new(0, 0, 120, 40);
        let layout = app_body_layout(area, 24);
        assert_eq!(layout.explorer.width, 24);
        assert_eq!(layout.v_splitter.width, 1);
        assert_eq!(layout.v_splitter.x, 24);
        assert_eq!(layout.workspace.x, 25);
        assert!(layout.explorer.x < layout.workspace.x);
        assert_eq!(layout.explorer.y, layout.workspace.y);
    }

    #[test]
    fn layout_clamps_wide_explorer_keeps_workspace() {
        let area = Rect::new(0, 0, 40, 40);
        // Even with a huge requested width, the workspace keeps 4 columns.
        let layout = app_body_layout(area, 100);
        assert_eq!(layout.explorer.width, 40 - 1 - 4);
        assert!(layout.workspace.width >= 4);
    }

    #[test]
    fn layout_tiny_area_returns_empty() {
        assert_eq!(
            app_body_layout(Rect::new(0, 0, 5, 40), 24),
            AppBodyLayout::default()
        );
    }

    #[test]
    fn hit_detects_splitter_column() {
        let area = Rect::new(0, 0, 120, 40);
        let layout = app_body_layout(area, 24);
        assert!(splitter_at(
            &layout,
            layout.v_splitter.x,
            layout.v_splitter.y
        ));
        assert!(!splitter_at(&layout, 10, 10));
    }

    #[test]
    fn explorer_width_from_x_clamps() {
        let area = Rect::new(0, 0, 120, 40);
        assert_eq!(explorer_width_for_x(area, 40), 40);
        assert_eq!(explorer_width_for_x(area, 1000), 120 - 1 - 4);
        assert_eq!(explorer_width_for_x(area, 0), MIN_EXPLORER_WIDTH);
    }
}
