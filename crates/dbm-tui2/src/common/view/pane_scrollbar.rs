//! Scrollbar rendering helpers (theme-aware thumb/track via [`Palette`]).
//! The track/thumb geometry and hit-testing live in
//! `crate::common::layout::pane_scrollbar` — this module only draws.
//!
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};

use super::theme::Palette;

/// Track/thumb styles for a scrollbar, derived from the semantic palette.
#[derive(Debug, Clone, Copy)]
pub struct ScrollbarStyle {
    pub track: Style,
    pub thumb: Style,
}

pub fn results_scrollbar_style(palette: &Palette, dragging: bool) -> ScrollbarStyle {
    ScrollbarStyle {
        track: Style::default().fg(palette.border),
        thumb: Style::default().fg(if dragging {
            palette.accent
        } else {
            palette.muted
        }),
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
    fn results_scrollbar_style_has_distinct_track_and_thumb() {
        let p = palette();
        let idle = results_scrollbar_style(&p, false);
        let drag = results_scrollbar_style(&p, true);
        assert_ne!(idle.thumb, drag.thumb);
    }
}
