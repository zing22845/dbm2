//! Pure layout for the SQL tab body.
//!
//! The single source of truth for where each pane and splitter sits. Both the
//! view (to render) and the run loop (to hit-test mouse drags) call
//! [`sql_tab_layout`], so a splitter can only ever be found where it is drawn.
//!
//! Mirrors the original dbm `sql_tab_layout` (ui.rs §11): a vertical split puts
//! the editor+history row on top and results on the bottom; the top row is a
//! horizontal split with the SQL editor on the left and the history on the
//! right. The split positions come from the tab's stored ratio/width, clamped
//! to the current track so neither pane can collapse.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::features::sql_workspace::sql_tab::splitter::state::MIN_SQL_PANE_WIDTH;

/// The panes and splitter strips computed by [`sql_tab_layout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SqlTabLayout {
    pub editor: Rect,
    pub history: Rect,
    pub results: Rect,
    /// The 1-row horizontal splitter between the top row and results.
    pub h_splitter: Rect,
    /// The 1-column vertical splitter between editor and history.
    pub v_splitter: Rect,
    /// Actual editor+history row height bounds (rows), what the layout clamped
    /// to. Nudge/drag read these so the stored value and the rendered split
    /// always agree (no redundant repaint at the boundary).
    pub editor_top_min: u16,
    pub editor_top_max: u16,
    /// Actual history width bounds (cols), what the layout clamped to.
    pub history_min: u16,
    pub history_max: u16,
}

/// Compute the SQL tab body layout. `area` is the region below the tab bar.
///
/// `editor_top_height` is the stored editor+history row height **in rows**; it
/// is clamped to `[20%, 80%]` of the current track here (so a height recorded
/// on a taller terminal is re-clamped correctly after a resize).
pub fn sql_tab_layout(area: Rect, editor_top_height: u16, history_width: u16) -> SqlTabLayout {
    let empty = SqlTabLayout::default();

    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0), // top row (editor + history)
            Constraint::Length(1), // horizontal splitter
            Constraint::Min(0), // results
        ])
        .split(area);
    if body.len() != 3 || body[0].height < 2 || body[2].height < 2 {
        return empty;
    }

    // Editor top-pane height in rows, clamped to [20%, 80%] of the track so
    // neither the top row nor results collapses. The bounds are exported so the
    // nudge/drag clamp to the exact values the layout accepts.
    let track_h = body[0].height + 1 + body[2].height;
    let editor_top_min = (track_h * 20) / 100;
    let editor_top_max = track_h.saturating_sub(1).saturating_sub(editor_top_min);
    let row_h = editor_top_height.clamp(editor_top_min, editor_top_max);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(row_h),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);
    if vertical.len() != 3 {
        return empty;
    }
    let (top_row, h_splitter, results) = (vertical[0], vertical[1], vertical[2]);
    if top_row.height < 2 {
        return empty;
    }

    // Top row horizontal split: editor (left) + vertical splitter + history.
    let track_w = top_row.width;
    // The editor always keeps MIN_SQL_PANE_WIDTH (plus the 1-col splitter), so
    // history maxes out at `track_w - (MIN_SQL_PANE_WIDTH + 1)`. The bounds are
    // exported so the history nudge/drag clamp to the exact values the layout
    // accepts (stored and rendered widths never disagree).
    let history_min = 12u16;
    let history_max = track_w.saturating_sub(MIN_SQL_PANE_WIDTH + 1).max(history_min);
    let history_w = history_width.clamp(history_min, history_max);
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(history_w),
        ])
        .split(top_row);
    if top.len() != 3 || top[0].width < 1 {
        return empty;
    }
    let (editor, v_splitter, history) = (top[0], top[1], top[2]);

    SqlTabLayout {
        editor,
        history,
        results,
        h_splitter,
        v_splitter,
        editor_top_min,
        editor_top_max,
        history_min,
        history_max,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_places_all_panes() {
        let area = Rect::new(0, 0, 120, 40);
        let layout = sql_tab_layout(area, 18, 24);
        assert!(layout.editor.width > 0 && layout.editor.height > 0);
        assert!(layout.history.width > 0 && layout.history.height > 0);
        assert!(layout.results.height > 0);
        // Editor left of history, both in the top row above results.
        assert!(layout.editor.x < layout.history.x);
        assert!(layout.editor.y == layout.history.y);
        assert!(layout.editor.y + layout.editor.height <= layout.results.y);
        // Splitter rows have width/height 1.
        assert_eq!(layout.h_splitter.height, 1);
        assert_eq!(layout.v_splitter.width, 1);
    }

    #[test]
    fn layout_tiny_area_returns_empty() {
        assert_eq!(
            sql_tab_layout(Rect::new(0, 0, 5, 2), 18, 24),
            SqlTabLayout::default()
        );
    }
}
