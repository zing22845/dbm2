//! Shared scrollbar rendering, layout, and pointer mapping for scrollable panes.
//!
//! Theme-aware via the semantic `Palette`: the track uses the muted/border slot
//! and the thumb uses the accent slot, brightening when actively dragged.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};

use super::theme::Palette;

/// Which pane scrollbar is being dragged (single active drag — not per-pane bools).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveScrollbar {
    TreeV,
    TreeH,
    ObjectsV,
    ObjectsH,
    ResultsV,
    ResultsH,
    ResultsDetailV,
    HistoryV,
    HistoryH,
    HistoryDetailV,
    SqlV,
    OverviewV,
    OverviewH,
    DiscoverTargetsV,
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

/// Map a pointer position along a scrollbar track to a scroll offset in `0..=max_scroll`.
pub fn scroll_offset_from_track(
    pointer: u16,
    track_start: u16,
    track_len: u16,
    max_scroll: u32,
) -> u32 {
    if track_len == 0 || max_scroll == 0 {
        return 0;
    }
    let track_len = u32::from(track_len.max(1));
    let rel = u32::from(pointer.saturating_sub(track_start).min(track_len as u16));
    (rel * max_scroll) / track_len
}

pub fn point_in_bar(bar: Rect, x: u16, y: u16) -> bool {
    bar.width > 0
        && bar.height > 0
        && x >= bar.x
        && x < bar.x.saturating_add(bar.width)
        && y >= bar.y
        && y < bar.y.saturating_add(bar.height)
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
        crate::common::view::theme::dracula().palette().clone()
    }

    #[test]
    fn scroll_offset_from_track_maps_ends() {
        assert_eq!(scroll_offset_from_track(10, 10, 10, 100), 0);
        assert_eq!(scroll_offset_from_track(19, 10, 10, 100), 90);
        assert_eq!(scroll_offset_from_track(10, 10, 10, 0), 0);
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
