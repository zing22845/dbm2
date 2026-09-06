//! App body layout: the explorer / splitter / workspace columns.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use super::state::{MAX_EXPLORER_WIDTH, MIN_EXPLORER_WIDTH};

/// The panes and splitter strip computed by [`app_body_layout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AppBodyLayout {
    /// The Explorer pane (left).
    pub explorer: Rect,
    /// The workspace region (right), holding the SQL / instance workspace.
    pub workspace: Rect,
    /// The 1-column vertical splitter between explorer and workspace.
    pub v_splitter: Rect,
}

/// Compute the app body layout: explorer (left) + vertical splitter +
/// workspace (right). `explorer_width` is the stored column width, clamped so
/// the workspace always keeps a minimum and the explorer a minimum. Returns an
/// empty layout when the track is too small to place both panes.
pub fn app_body_layout(area: Rect, explorer_width: u16) -> AppBodyLayout {
    let empty = AppBodyLayout::default();
    if area.width < MIN_EXPLORER_WIDTH + 1 + 1 {
        return empty;
    }
    // The workspace must keep at least a few columns; the explorer width is
    // clamped so it cannot swallow the whole track.
    let min_workspace = 4u16;
    let max_explorer = area.width.saturating_sub(1).saturating_sub(min_workspace);
    let explorer_w = explorer_width
        .clamp(MIN_EXPLORER_WIDTH, MAX_EXPLORER_WIDTH)
        .min(max_explorer);
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(explorer_w),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);
    if panes.len() != 3 || panes[2].width < min_workspace {
        return empty;
    }
    AppBodyLayout {
        explorer: panes[0],
        v_splitter: panes[1],
        workspace: panes[2],
    }
}
