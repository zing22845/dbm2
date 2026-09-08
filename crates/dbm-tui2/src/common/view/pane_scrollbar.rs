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

/// All vertical and horizontal scrollbars share this style, so the palette's
/// dedicated `scrollbar_inactive` / `scrollbar_active` slots (original dbm's
/// idle / drag thumb colours) tune every scrollbar from one place. The track
/// keeps its own dim border-derived colour so an idle thumb still reads as the
/// brighter bar over it.
pub fn results_scrollbar_style(palette: &Palette, dragging: bool) -> ScrollbarStyle {
    ScrollbarStyle {
        track: Style::default().fg(palette.border),
        thumb: Style::default().fg(if dragging {
            palette.scrollbar_active
        } else {
            palette.scrollbar_inactive
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
    fn results_scrollbar_style_uses_the_dedicated_palette_slots() {
        let p = palette();
        let idle = results_scrollbar_style(&p, false);
        let drag = results_scrollbar_style(&p, true);
        // Idle / drag thumbs come from the dedicated scrollbar slots.
        assert_eq!(idle.thumb.fg, Some(p.scrollbar_inactive));
        assert_eq!(drag.thumb.fg, Some(p.scrollbar_active));
        assert_ne!(idle.thumb, drag.thumb);
        // Track keeps its own dim colour (visible idle thumb over the track).
        assert_ne!(idle.thumb, idle.track);
        // The inactive slot is a dedicated colour, not muted/accent (the dark
        // drag yellow legitimately equals accent — that is the original dbm's
        // exact colour, and scrollbars read their own slot so tuning it never
        // touches other chrome).
        assert_ne!(p.scrollbar_inactive, p.muted);
        assert_ne!(p.scrollbar_inactive, p.accent);
        assert_ne!(p.scrollbar_inactive, p.scrollbar_active);
    }
}
