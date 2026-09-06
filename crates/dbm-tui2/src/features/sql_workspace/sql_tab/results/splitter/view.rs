//! Geometry of the Results-internal detail/list splitter.
//!
//! Given the Results Block's outer area and `detail_open`, this module exposes
//! helpers to hit-test the splitter strip and translate a mouse-x drag into a
//! new detail pane width. The render path mirrors `history::splitter::view`.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::common::view::splitter::{SplitOrientation, draw};

use super::state::{SPLITTER_WIDTH, clamp_detail_pane_width};

/// Split the Results Block's `inner` area into list + splitter + detail.
/// Returns `(list_inner, splitter_rect, detail_inner)` when `detail_open`,
/// or `(inner, None, None)` when detail is closed.
pub fn split_inner(
    inner: Rect,
    detail_open: bool,
    detail_pane_width: u16,
) -> (Rect, Option<Rect>, Option<Rect>) {
    if !detail_open {
        return (inner, None, None);
    }
    let detail_w = clamp_detail_pane_width(detail_pane_width).min(inner.width.saturating_sub(2));
    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(SPLITTER_WIDTH),
            Constraint::Length(detail_w),
        ])
        .split(inner);
    (split[0], Some(split[1]), Some(split[2]))
}

/// Compute the Results Block **inner** area from its `outer` Rect (subtracts
/// 1 column of borders on all sides).
pub fn block_inner(outer: Rect) -> Rect {
    Rect {
        x: outer.x.saturating_add(1),
        y: outer.y.saturating_add(1),
        width: outer.width.saturating_sub(2),
        height: outer.height.saturating_sub(2),
    }
}

/// The rect of the Results detail/list internal splitter, if the detail is
/// open. `outer` is the full Results Block rect (before border subtraction).
/// The splitter spans only the content band (above the pagination toolbar and
/// the footer), so its height stays consistent with the drawn splitter.
#[allow(clippy::too_many_arguments)]
pub fn results_detail_splitter(
    outer: Rect,
    detail_open: bool,
    detail_pane_width: u16,
    row_count: usize,
    search_active: bool,
    sql_status: &str,
) -> Option<Rect> {
    if !detail_open {
        return None;
    }
    let inner = block_inner(outer);
    let layout = crate::features::sql_workspace::sql_tab::results::layout::compute_results_layout(
        inner,
        true,
        detail_pane_width,
        row_count,
        search_active,
        sql_status,
    );
    layout.splitter
}

/// Compute the detail pane width for a drag of the Results detail/list
/// splitter at absolute x-coordinate `point_x`. The detail occupies the right
/// side, so its width = distance from the pointer to the inner area's right
/// edge (with the splitter's own 1 col accounted for by clamping).
pub fn detail_width_for_x(inner: Rect, detail_pane_width: u16, point_x: u16) -> u16 {
    let _ = detail_pane_width;
    inner.right().saturating_sub(point_x)
}

/// Render the splitter strip with hover/drag highlight.
pub fn render(frame: &mut Frame, splitter: Rect, hover: bool, dragging: bool) {
    draw(frame, splitter, SplitOrientation::Vertical, hover, dragging);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sql_workspace::sql_tab::results::splitter::state::MIN_DETAIL_PANE_WIDTH;

    #[test]
    fn split_inner_without_detail_returns_whole_inner() {
        let inner = Rect::new(1, 1, 80, 24);
        let (list, splitter, detail) = split_inner(inner, false, 40);
        assert_eq!(list, inner);
        assert!(splitter.is_none());
        assert!(detail.is_none());
    }

    #[test]
    fn split_inner_with_detail_splits_three_ways() {
        let inner = Rect::new(1, 1, 80, 24);
        let (list, splitter, detail) = split_inner(inner, true, 40);
        assert_eq!(list.width + 1 + detail.unwrap().width, 80);
        assert_eq!(splitter.unwrap().width, 1);
        assert_eq!(splitter.unwrap().y, 1);
    }

    #[test]
    fn detail_width_is_clamped() {
        let inner = Rect::new(1, 1, 80, 24);
        let (_list, _splitter, detail) = split_inner(inner, true, 10);
        assert_eq!(detail.unwrap().width, MIN_DETAIL_PANE_WIDTH);
    }

    #[test]
    fn splitter_rect_uses_block_inner() {
        let outer = Rect::new(0, 3, 120, 20);
        let splitter = results_detail_splitter(outer, true, 40, 1, false, "").unwrap();
        // outer minus 1-col border → inner starts at x=1, splitter is at x=1+list_w
        // where list_w = 120 - 2 - 1 - 40 = 77, so splitter at x=78
        assert_eq!(splitter.x, 78);
        assert_eq!(splitter.width, 1);
    }

    #[test]
    fn detail_width_for_x_grows_when_pointer_moves_right() {
        let inner = Rect::new(1, 1, 80, 24);
        let w_at_50 = detail_width_for_x(inner, 40, 50);
        let w_at_60 = detail_width_for_x(inner, 40, 60);
        assert!(w_at_50 > w_at_60, "more right → smaller detail width");
    }
}
