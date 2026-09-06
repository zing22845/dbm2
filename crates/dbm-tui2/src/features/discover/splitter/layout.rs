//! Discover body layout: the targets editor, the splitter row and the
//! results list.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

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
