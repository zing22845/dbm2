//! Clear an overlay rect without letting double-width glyphs (CJK, etc.) from
//! the layer below straddle the left/right edges.
//!
//! Upstream: <https://github.com/ratatui/ratatui/issues/2526>

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Clear, Widget};
use unicode_width::UnicodeWidthStr;

/// Like [`Clear`], but also shrinks a wide glyph at `area.x - 1` and blanks an
/// orphaned continuation at `area.right()` so popup borders are not eaten by
/// underlying CJK/emoji cells.
pub fn clear_overlay(frame: &mut Frame, area: Rect) {
    clear_overlay_buf(frame.buffer_mut(), area);
}

pub fn clear_overlay_buf(buf: &mut Buffer, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let bounds = *buf.area();
    let top = area.top().max(bounds.top());
    let bottom = area.bottom().min(bounds.bottom());
    if top >= bottom {
        return;
    }

    // Left edge: a width-2 glyph at area.x-1 still paints into area.x.
    if area.x > bounds.x {
        let x = area.x - 1;
        for y in top..bottom {
            let cell = &mut buf[(x, y)];
            if cell.symbol().width() > 1 {
                cell.reset();
            }
        }
    }

    // Right edge: clearing the left half of a wide glyph leaves "" at area.right().
    let right = area.right();
    if right < bounds.right() {
        let inner_right = right - 1;
        for y in top..bottom {
            if buf[(inner_right, y)].symbol().width() > 1 {
                buf[(right, y)].reset();
            }
        }
    }

    Clear.render(area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    #[test]
    fn left_edge_double_width_replaced_with_space() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        buf.set_string(0, 0, "abc字", Style::default());
        clear_overlay_buf(&mut buf, Rect::new(4, 0, 2, 1));
        assert_eq!(buf[(3, 0)].symbol(), " ");
        assert_eq!(buf[(4, 0)].symbol(), " ");
        assert_eq!(buf[(5, 0)].symbol(), " ");
    }

    #[test]
    fn left_edge_single_width_left_alone() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        buf.set_string(0, 0, "abcdef", Style::default());
        clear_overlay_buf(&mut buf, Rect::new(4, 0, 2, 1));
        assert_eq!(buf[(3, 0)].symbol(), "d");
        assert_eq!(buf[(4, 0)].symbol(), " ");
        assert_eq!(buf[(5, 0)].symbol(), " ");
    }

    #[test]
    fn right_edge_continuation_cleared() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        buf.set_string(0, 0, "abc字d", Style::default());
        // "字" occupies cols 3..4; clear cols 0..4 leaves continuation at 4.
        assert_eq!(buf[(3, 0)].symbol(), "字");
        clear_overlay_buf(&mut buf, Rect::new(0, 0, 4, 1));
        assert_eq!(buf[(3, 0)].symbol(), " ");
        assert_eq!(buf[(4, 0)].symbol(), " ");
        assert_eq!(buf[(5, 0)].symbol(), "d");
    }

    #[test]
    fn right_edge_with_no_overhang_leaves_neighbour_alone() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        buf.set_string(0, 0, "abcdef", Style::default());
        clear_overlay_buf(&mut buf, Rect::new(0, 0, 4, 1));
        assert_eq!(buf[(4, 0)].symbol(), "e");
        assert_eq!(buf[(5, 0)].symbol(), "f");
    }

    #[test]
    fn area_at_buffer_edges_does_not_panic() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        buf.set_string(0, 0, "测试", Style::default());
        clear_overlay_buf(&mut buf, Rect::new(0, 0, 4, 2));
        clear_overlay_buf(&mut buf, Rect::new(0, 0, 0, 0));
    }
}
