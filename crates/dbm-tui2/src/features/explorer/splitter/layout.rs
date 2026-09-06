//! Explorer body layout: the instances tree, the splitter row and the
//! objects tree.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

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
