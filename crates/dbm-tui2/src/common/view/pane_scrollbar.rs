//! Shared scrollbar rendering, layout, and pointer mapping for scrollable panes.
//!
//! Theme-aware via the semantic `Palette`: the track uses the muted/border slot
//! and the thumb uses the accent slot, brightening when actively dragged.
//!
//! Also provides the shared `discover_anchor` viewport calculation and generic
//! scrollbar hit-test helpers that every feature reuses — keeping anchor math,
//! thumb geometry, and drag behavior consistent across the whole app.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};

use super::theme::Palette;

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
            Self::TreeH
            | Self::ObjectsH
            | Self::ResultsH
            | Self::HistoryH
            | Self::OverviewH => ScrollAxis::Horizontal,
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
        scroll_offset_from_track(pointer, self.track_start, self.viewport_len, self.max_scroll)
    }
}

/// Track/thumb styles for a scrollbar, derived from the semantic palette.
#[derive(Debug, Clone, Copy)]
pub struct ScrollbarStyle {
    pub track: Style,
    pub thumb: Style,
}

pub fn results_scrollbar_style(palette: &Palette, dragging: bool) -> ScrollbarStyle {
    ScrollbarStyle {
        track: Style::default().fg(palette.border),
        thumb: Style::default()
            .fg(if dragging { palette.accent } else { palette.muted }),
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
    let rel = pointer.saturating_sub(track_start).min(track_len.saturating_sub(1) as u16) as usize;
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

/// Discover-style cursor anchor: the viewport's start row is only pushed
/// when the cursor would fall OUTSIDE the current window. Cursor moving
/// inside the window does NOT move the viewport. When `scroll_locked` is
/// true (manual v_scrollbar drag), the anchor is completely skipped so the
/// drag position is preserved.
///
/// All 7 scrollable panes (instances, objects, discover targets/results,
/// iw connections/overview, sql history) MUST use this function so their
/// anchor math stays identical.
pub fn discover_anchor(
    scroll: usize,
    max_scroll: usize,
    cursor: usize,
    viewport: usize,
    scroll_locked: bool,
) -> usize {
    let viewport = viewport.max(1);
    let mut start = scroll.min(max_scroll);
    if !scroll_locked {
        if cursor < start {
            start = cursor;
        } else if cursor >= start + viewport {
            start = cursor + 1 - viewport;
        }
    }
    start
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

pub fn draw_vertical_pane_scrollbar(
    frame: &mut Frame,
    area: Rect,
    scroll_pos: usize,
    viewport_len: usize,
    max_scroll: usize,
    palette: &Palette,
    dragging: bool,
) {
    if area.width == 0 || area.height == 0 || max_scroll == 0 {
        return;
    }
    let style = results_scrollbar_style(palette, dragging);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("┊"))
        .thumb_symbol("█")
        .track_style(style.track)
        .thumb_style(style.thumb);
    let mut state = ScrollbarState::new(max_scroll.saturating_add(1))
        .position(scroll_pos)
        .viewport_content_length(viewport_len);
    frame.render_stateful_widget(scrollbar, area, &mut state);
}

pub fn draw_horizontal_pane_scrollbar(
    frame: &mut Frame,
    area: Rect,
    scroll_pos: usize,
    viewport_len: usize,
    max_scroll: usize,
    palette: &Palette,
    dragging: bool,
) {
    if area.width == 0 || area.height == 0 || max_scroll == 0 {
        return;
    }
    let style = results_scrollbar_style(palette, dragging);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("─"))
        .thumb_symbol("█")
        .track_style(style.track)
        .thumb_style(style.thumb);
    let mut state = ScrollbarState::new(max_scroll.saturating_add(1))
        .position(scroll_pos)
        .viewport_content_length(viewport_len);
    frame.render_stateful_widget(scrollbar, area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        crate::common::view::theme::default().palette().clone()
    }

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
    fn results_scrollbar_style_has_distinct_track_and_thumb() {
        let p = palette();
        let idle = results_scrollbar_style(&p, false);
        let drag = results_scrollbar_style(&p, true);
        assert_ne!(idle.thumb, drag.thumb);
    }
}
