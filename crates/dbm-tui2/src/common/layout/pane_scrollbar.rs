//! Shared scrollbar geometry and pointer mapping: the unified `pane_anchor`
//! viewport calculation ([`RowHeights`]), the per-pane scrollbar track layout
//! ([`PaneScrollLayout`]) and the generic hit-test helpers ([`v_scrollbar_hit`],
//! [`h_scrollbar_hit`], [`point_in_bar`]). Pure layout math — no rendering.
//!
//! `ActiveScrollbar` / [`ScrollbarDrag`] also live here: they are the single
//! active-drag model shared by `AppState`, the mouse layer and the renderers.
//!

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Which pane scrollbar is being dragged (single active drag — not per-pane bools).
///
/// The `H`/`V` suffix names the axis the scrollbar scrolls along, which
/// [`ActiveScrollbar::axis`] reports so the drag handler can map the pointer to
/// an offset without a per-variant branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveScrollbar {
    /// Explorer instances tree, vertical.
    TreeV,
    /// Explorer instances tree, horizontal.
    TreeH,
    /// Explorer objects tree, vertical.
    ObjectsV,
    /// Explorer objects tree, horizontal.
    ObjectsH,
    /// SQL results grid, vertical.
    ResultsV,
    /// SQL results grid, horizontal.
    ResultsH,
    /// SQL results detail sub-pane, vertical.
    ResultsDetailV,
    /// SQL history list, vertical.
    HistoryV,
    /// SQL history list, horizontal.
    HistoryH,
    /// SQL history detail sub-pane, vertical.
    HistoryDetailV,
    /// SQL editor body, vertical.
    SqlV,
    /// Instance workspace overview, vertical.
    OverviewV,
    /// Instance workspace overview, horizontal.
    OverviewH,
    /// Discover targets list, vertical.
    DiscoverTargetsV,
    /// Discover results list, vertical.
    DiscoverResultsV,
    /// Instance workspace connections list, vertical.
    ConnectionsV,
}

/// The axis a scrollbar scrolls along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis {
    Horizontal,
    Vertical,
}

impl ActiveScrollbar {
    /// The axis this scrollbar scrolls along.
    pub fn axis(self) -> ScrollAxis {
        match self {
            Self::TreeH | Self::ObjectsH | Self::ResultsH | Self::HistoryH | Self::OverviewH => {
                ScrollAxis::Horizontal
            }
            Self::TreeV
            | Self::ObjectsV
            | Self::ResultsV
            | Self::ResultsDetailV
            | Self::HistoryV
            | Self::HistoryDetailV
            | Self::SqlV
            | Self::OverviewV
            | Self::DiscoverTargetsV
            | Self::DiscoverResultsV
            | Self::ConnectionsV => ScrollAxis::Vertical,
        }
    }
}

/// A scrollbar drag in progress: which one, plus the track geometry captured
/// at press time.
///
/// Held as a single value in `AppState` rather than one `Option` per scrollbar,
/// because only one drag can be active at a time and every render needs to know
/// *which* one so it can highlight that bar. The geometry is captured on Down
/// because the pointer routinely leaves the track mid-drag while the pointer →
/// offset mapping has to stay continuous.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbarDrag {
    /// Which scrollbar is being dragged.
    pub which: ActiveScrollbar,
    /// Track start along the scroll axis (x for horizontal, y for vertical).
    pub track_start: u16,
    /// Track length in pixels along the scroll axis.
    pub viewport_len: usize,
    /// Maximum scroll offset this scrollbar can reach.
    pub max_scroll: usize,
}

impl ScrollbarDrag {
    /// Map the current pointer position to a scroll offset, reading whichever
    /// coordinate matches this drag's axis (`x` for a horizontal bar, `y` for a
    /// vertical one). `scroll_offset_from_track` already clamps to `max_scroll`.
    pub fn offset_for_pointer(&self, x: u16, y: u16) -> usize {
        let pointer = match self.which.axis() {
            ScrollAxis::Horizontal => x,
            ScrollAxis::Vertical => y,
        };
        scroll_offset_from_track(
            pointer,
            self.track_start,
            self.viewport_len,
            self.max_scroll,
        )
    }
}

/// Map a pointer pixel along a scrollbar track to a scroll offset in
/// `0..=max_scroll`. Uses `track_len - 1` as the denominator so the last
/// pixel of the track maps exactly to `max_scroll` — matching how ratatui's
/// ScrollbarState reports position. Works for both vertical and horizontal
/// scrollbars (caller passes pointer y or x, and track height or width).
pub fn scroll_offset_from_track(
    pointer: u16,
    track_start: u16,
    track_len: usize,
    max_scroll: usize,
) -> usize {
    if track_len <= 1 || max_scroll == 0 {
        return 0;
    }
    let denom = track_len.saturating_sub(1);
    let rel = pointer
        .saturating_sub(track_start)
        .min(track_len.saturating_sub(1) as u16) as usize;
    let pos = (rel * max_scroll) / denom;
    pos.min(max_scroll)
}

/// Generic hit-test result for any scrollbar (vertical or horizontal).
/// `track_start` is the coordinate along the scroll axis (y for vertical,
/// x for horizontal). `track_len` is the track length in PIXELS — the drag
/// formula needs this, NOT data-row count.
#[derive(Debug, Clone, Copy)]
pub struct ScrollbarHitInfo {
    pub track_start: u16,
    pub track_len: usize,
    pub max_scroll: usize,
}

/// Generic vertical scrollbar hit-test — reuses the pane's own
/// `PaneScrollLayout` to stay in sync with what the renderer drew.
pub fn v_scrollbar_hit(
    layout: &PaneScrollLayout,
    max_scroll: usize,
    x: u16,
    y: u16,
) -> Option<ScrollbarHitInfo> {
    let v_bar = layout.v_scrollbar?;
    if max_scroll == 0 {
        return None;
    }
    if !point_in_bar(v_bar, x, y) {
        return None;
    }
    Some(ScrollbarHitInfo {
        track_start: v_bar.y,
        track_len: v_bar.height.max(1) as usize,
        max_scroll,
    })
}

/// Generic horizontal scrollbar hit-test.
pub fn h_scrollbar_hit(
    layout: &PaneScrollLayout,
    max_scroll: usize,
    x: u16,
    y: u16,
) -> Option<ScrollbarHitInfo> {
    let h_bar = layout.h_scrollbar?;
    if max_scroll == 0 {
        return None;
    }
    if !point_in_bar(h_bar, x, y) {
        return None;
    }
    Some(ScrollbarHitInfo {
        track_start: h_bar.x,
        track_len: h_bar.width.max(1) as usize,
        max_scroll,
    })
}

pub fn point_in_bar(bar: Rect, x: u16, y: u16) -> bool {
    bar.width > 0
        && bar.height > 0
        && x >= bar.x
        && x < bar.x.saturating_add(bar.width)
        && y >= bar.y
        && y < bar.y.saturating_add(bar.height)
}

/// Per-step heights along a scroll axis.
///
/// A "step" is one logical unit of content — a data row for vertical scroll, a
/// column for horizontal scroll. Every scrollable pane anchors through
/// [`pane_anchor`] using one of these, so there is a single source of truth for
/// scroll math: non-wrapping panes use [`RowHeights::Uniform(1)`] (one terminal
/// row per data row) and wrapping panes use [`RowHeights::Variable`] (a wrapped
/// row occupies several terminal rows). No pane scrolls "by data row" while
/// another scrolls "by pixel" — the unit is always a logical step; only the
/// step's extent (height) varies.
#[derive(Debug, Clone, Copy)]
pub enum RowHeights<'a> {
    /// Every step has height `step` (e.g. `1` terminal row / column). Zero alloc.
    Uniform(usize),
    /// Per-step heights; `heights.len()` MUST equal `total`.
    Variable(&'a [usize]),
}

/// Result of [`pane_anchor`]: the anchored viewport for one scroll axis.
#[derive(Debug, Clone, Copy)]
pub struct PaneAnchor {
    /// Index of the first visible logical step (top row / leftmost column).
    pub start: usize,
    /// Maximum `start` the scrollbar can reach (largest top index whose full
    /// window still fits the content extent — the bottom step is fully visible,
    /// never split across the edge).
    pub max_scroll: usize,
    /// Number of logical steps that fit in the viewport starting at `start`
    /// without splitting a step across the top/bottom boundary.
    pub visible: usize,
}

/// Unified viewport anchor for every scrollable pane.
///
/// Anchors the viewport to `scroll` (the draggable / manually-set position),
/// pushing it only when `cursor` would fall outside the window — and skipped
/// entirely when `scroll_locked` is true (an in-progress scrollbar drag
/// preserves the manual position). Always aligns to logical-step boundaries: a
/// step is shown whole or not at all, which is exactly what prevents cursor
/// drift in wrapping panes (e.g. overview, whose rows wrap onto several
/// terminal rows).
///
/// `content_extent` is the viewport size along the scroll axis (terminal rows
/// for vertical scroll, columns for horizontal). `total` is the number of
/// steps. `heights` describes each step's extent (see [`RowHeights`]).
pub fn pane_anchor(
    total: usize,
    heights: RowHeights,
    content_extent: usize,
    scroll: usize,
    cursor: usize,
    scroll_locked: bool,
) -> PaneAnchor {
    let ext = content_extent.max(1);
    if total == 0 {
        return PaneAnchor {
            start: 0,
            max_scroll: 0,
            visible: 0,
        };
    }

    match heights {
        RowHeights::Uniform(step) => {
            let step = step.max(1);
            let total_ext = total * step;
            // Largest start with start*step <= total_ext - ext.
            let max_start = if total_ext <= ext {
                0
            } else {
                total.saturating_sub(ext.div_ceil(step))
            };
            let viewport_units = ext / step;
            let cursor = cursor.min(total - 1);
            let mut start = scroll.min(max_start);
            if !scroll_locked {
                if cursor < start {
                    start = cursor;
                } else if cursor >= start + viewport_units {
                    start = cursor + 1 - viewport_units;
                }
            }
            start = start.min(max_start);
            let visible = (total - start).min(viewport_units);
            PaneAnchor {
                start,
                max_scroll: max_start,
                visible,
            }
        }
        RowHeights::Variable(h) => {
            // Cumulative tops: cum[0]=0, cum[i]=sum of heights[0..i].
            let mut cum = vec![0usize; total + 1];
            for i in 0..total {
                cum[i + 1] = cum[i] + h.get(i).copied().unwrap_or(0);
            }
            let total_ext = cum[total];
            // Largest start with cum[start] <= total_ext - ext.
            let max_pixel = total_ext.saturating_sub(ext);
            // `cum` is ascending and cum[0] == 0 <= max_pixel, so the count is
            // at least 1 and the last qualifying index is count - 1.
            let max_start = cum
                .iter()
                .take_while(|&&top| top <= max_pixel)
                .count()
                .saturating_sub(1);
            let cursor = cursor.min(total - 1);
            let mut start = scroll.min(max_start);
            if !scroll_locked {
                let win_top = cum[start];
                let win_bottom = win_top + ext;
                let cur_top = cum[cursor];
                let cur_bottom = cum[cursor + 1];
                // Only push when the cursor is not already fully inside the window.
                if cur_top < win_top {
                    start = cursor;
                } else if cur_bottom > win_bottom {
                    let lo = cur_bottom.saturating_sub(ext);
                    let mut s = 0usize;
                    while s < total && cum[s] < lo {
                        s += 1;
                    }
                    start = s.min(cursor);
                }
            }
            start = start.min(max_start);
            // Steps from `start` that fit in `ext` without crossing the edge.
            let mut visible = 0usize;
            let mut px = 0usize;
            let mut i = start;
            while i < total {
                let hh = h.get(i).copied().unwrap_or(0);
                if px + hh > ext {
                    break;
                }
                px += hh;
                visible += 1;
                i += 1;
            }
            // Degenerate (a step taller than the pane): show at least the top step.
            if visible == 0 && start < total {
                visible = 1;
            }
            PaneAnchor {
                start,
                max_scroll: max_start,
                visible,
            }
        }
    }
}

pub struct PaneScrollLayout {
    pub content_area: Rect,
    pub h_scrollbar: Option<Rect>,
    pub v_scrollbar: Option<Rect>,
}

/// Reserve vertical and/or horizontal scrollbar tracks when content exceeds the viewport.
pub fn pane_scroll_layout(
    content: Rect,
    content_width: u16,
    row_count: usize,
    viewport_rows: usize,
) -> PaneScrollLayout {
    let needs_v = row_count > viewport_rows && viewport_rows > 0;
    let (main, v_bar) = if needs_v {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(content);
        (chunks[0], Some(chunks[1]))
    } else {
        (content, None)
    };

    let needs_h = content_width > main.width;
    let (content_area, h_bar) = if needs_h {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(main);
        (chunks[0], Some(chunks[1]))
    } else {
        (main, None)
    };

    PaneScrollLayout {
        content_area,
        h_scrollbar: h_bar,
        v_scrollbar: v_bar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_offset_from_track_maps_ends() {
        // With track_len=10, denom=9. First pixel → 0, last pixel → max_scroll.
        assert_eq!(scroll_offset_from_track(10, 10, 10, 100), 0);
        assert_eq!(scroll_offset_from_track(19, 10, 10, 100), 100);
        assert_eq!(scroll_offset_from_track(10, 10, 10, 0), 0);
        // Single-pixel track or zero max → 0.
        assert_eq!(scroll_offset_from_track(5, 5, 1, 100), 0);
    }

    #[test]
    fn pane_scroll_layout_reserves_both_bars_when_needed() {
        let content = Rect::new(0, 0, 30, 20);
        let layout = pane_scroll_layout(content, 40, 30, 10);
        assert!(layout.v_scrollbar.is_some());
        assert!(layout.h_scrollbar.is_some());
        assert_eq!(layout.content_area.width, 29);
        assert_eq!(layout.content_area.height, 19);
    }

    #[test]
    fn pane_scroll_layout_none_when_fits() {
        let content = Rect::new(0, 0, 30, 20);
        let layout = pane_scroll_layout(content, 20, 5, 10);
        assert!(layout.v_scrollbar.is_none());
        assert!(layout.h_scrollbar.is_none());
    }

    #[test]
    fn pane_anchor_uniform_matches_data_row_semantics() {
        // Uniform(1) must behave exactly like the old data-row anchor:
        // cursor inside the window keeps the manual scroll; outside pushes it.
        let total = 30;
        let ext = 10;
        // cursor inside window, scroll elsewhere → keep scroll (no jump).
        let a = pane_anchor(total, RowHeights::Uniform(1), ext, 5, 12, false);
        assert_eq!(a.start, 5, "cursor 12 inside [5,15) must keep scroll=5");
        assert_eq!(a.max_scroll, 20);
        assert_eq!(a.visible, 10);
        // cursor above window → snap to cursor.
        let a = pane_anchor(total, RowHeights::Uniform(1), ext, 5, 2, false);
        assert_eq!(a.start, 2);
        // cursor below window → push so it sits at the bottom.
        let a = pane_anchor(total, RowHeights::Uniform(1), ext, 5, 28, false);
        assert_eq!(a.start, 19, "cursor 28 -> start 28-10+1=19");
        // scroll_locked preserves manual scroll even if cursor is elsewhere.
        let a = pane_anchor(total, RowHeights::Uniform(1), ext, 7, 0, true);
        assert_eq!(a.start, 7);
        // scroll past max clamps, and a cursor inside that clamped window is kept.
        let a = pane_anchor(total, RowHeights::Uniform(1), ext, 999, 25, false);
        assert_eq!(
            a.start, 20,
            "scroll clamps to max_scroll; cursor 25 inside [20,30) stays"
        );
    }

    #[test]
    fn pane_anchor_uniform_no_overflow_when_total_small() {
        let a = pane_anchor(3, RowHeights::Uniform(1), 10, 0, 0, false);
        assert_eq!(a.start, 0);
        assert_eq!(a.max_scroll, 0);
        assert_eq!(a.visible, 3);
    }

    #[test]
    fn pane_anchor_variable_aligns_without_splitting_rows() {
        // heights: row0=1, row1=3 (wraps), row2=1, row3=2, row4=1. ext=4.
        let h = [1usize, 3, 1, 2, 1];
        let total = h.len();
        // cum=[0,1,4,5,7,8]; total_ext=8; max_pixel=4; largest cum[i]<=4 → i=2.
        let a = pane_anchor(total, RowHeights::Variable(&h), 4, 0, 0, false);
        assert_eq!(a.max_scroll, 2);
        // cursor=3 (row3, spans px 5..7) → window [cum[2]=4, 8) contains it; start=2.
        let a = pane_anchor(total, RowHeights::Variable(&h), 4, 0, 3, false);
        assert_eq!(a.start, 2);
        assert!(a.start + a.visible <= total);
        // visible window must not exceed the extent.
        let mut px = 0;
        for &row_h in &h[a.start..a.start + a.visible] {
            px += row_h;
        }
        assert!(px <= 4, "visible window must not exceed extent, got {px}");
    }

    #[test]
    fn pane_anchor_variable_degenerate_tall_step() {
        // A step taller than the pane still shows at least itself.
        let h = [10usize];
        let a = pane_anchor(1, RowHeights::Variable(&h), 4, 0, 0, false);
        assert_eq!(a.start, 0);
        assert_eq!(a.max_scroll, 0);
        assert_eq!(a.visible, 1);
    }
}
