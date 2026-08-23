//! Geometry of the discover targets/results horizontal splitter.
//!
//! Single source of truth for where the targets editor (top), the splitter
//! row, and the results list (bottom) sit. Both the view (to render) and the
//! run loop (to hit-test mouse drags) call [`discover_body_layout`].

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::common::view::splitter::clamp_split_px;
use crate::common::view::splitter::hit;

/// The panes computed by [`discover_body_layout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiscoverSplitLayout {
    /// The targets editor (top).
    pub targets: Rect,
    /// The 1-row horizontal splitter between targets and results.
    pub splitter: Rect,
    /// The results list (bottom).
    pub results: Rect,
    /// Actual targets-height bounds (rows), what the layout clamped to.
    pub targets_min: u16,
    pub targets_max: u16,
}

/// Compute the discover targets/results layout. `targets_height` is the stored
/// top-pane height in rows, clamped to `[20%, 80%]` of the current track (so a
/// height recorded on a taller terminal is re-clamped correctly after a
/// resize). Returns an empty layout when the track is too small.
pub fn discover_body_layout(area: Rect, targets_height: u16) -> DiscoverSplitLayout {
    let empty = DiscoverSplitLayout::default();
    if area.height < 3 {
        return empty;
    }
    // Recompute the track from the current area so the stored rows are clamped
    // against the *live* height (handles terminal resizes).
    let track_h = area.height;
    let targets_min = (track_h * 20) / 100;
    let targets_max = track_h.saturating_sub(1).saturating_sub(targets_min);
    let top_px = targets_height.clamp(targets_min, targets_max);
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
    DiscoverSplitLayout {
        targets: rows[0],
        splitter: rows[1],
        results: rows[2],
        targets_min,
        targets_max,
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
