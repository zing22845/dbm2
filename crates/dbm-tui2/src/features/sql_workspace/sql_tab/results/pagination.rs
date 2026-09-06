//! Results pagination: row limits, page navigation math and toolbar line.
//!
//! Pure page math lives here (independent of DB execution). The toolbar line
//! is theme-adaptive via the caller-provided styles.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

pub const DEFAULT_RESULTS_ROW_LIMIT: usize = 100;
pub const MIN_RESULTS_ROW_LIMIT: usize = 1;
pub const MAX_RESULTS_ROW_LIMIT: usize = 10_000;
pub const RESULTS_ROW_LIMIT_PRESETS: [usize; 4] = [50, 100, 500, 1000];
pub const RESULTS_PAGINATION_BAR_HEIGHT: u16 = 1;
/// Window (ms) within which a repeated `<` / `>` upgrades to first / last
/// page, mirroring the original dbm's `PAGE_CHORD_MS`.
pub const RESULTS_PAGE_CHORD_MS: u128 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultsPageAction {
    First,
    Prev,
    Next,
    Last,
    Set(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultsPaginationHit {
    RowLimit,
    CountTotalRows,
    FirstPage,
    PrevPage,
    PageNumber,
    NextPage,
    LastPage,
}

#[derive(Debug, Clone)]
pub struct ResultsPaginationLayout {
    pub bar_rect: Rect,
    pub hits: Vec<(ResultsPaginationHit, Rect)>,
}

/// Vim-style half/full viewport flip for the Results list (`Ctrl+f` / `Ctrl+b`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultsPageScrollPlan {
    MoveToRow(usize),
    NextPage,
    PrevPage,
    NoPlan,
}

pub fn page_count(total: u64, limit: usize) -> usize {
    if limit == 0 {
        return 1;
    }
    total.div_ceil(limit as u64).max(1) as usize
}

pub fn max_page(total: Option<u64>, limit: usize) -> Option<usize> {
    total.map(|t| page_count(t, limit))
}

pub fn clamp_page(page: usize, total: Option<u64>, limit: usize) -> usize {
    let page = page.max(1);
    match max_page(total, limit) {
        Some(max) => page.min(max),
        None => page,
    }
}

pub fn page_offset(page: usize, limit: usize) -> u64 {
    (page.saturating_sub(1) as u64) * limit as u64
}

pub fn can_go_next(
    page: usize,
    limit: usize,
    row_count: usize,
    total: Option<u64>,
    at_last_page: bool,
) -> bool {
    if at_last_page || row_count < limit {
        return false;
    }
    if let Some(total) = total {
        return (page as u64) * (limit as u64) < total;
    }
    true
}

pub fn parse_row_limit_input(input: &str) -> Option<usize> {
    let n: usize = input.trim().parse().ok()?;
    if (MIN_RESULTS_ROW_LIMIT..=MAX_RESULTS_ROW_LIMIT).contains(&n) {
        Some(n)
    } else {
        None
    }
}

pub fn parse_page_input(input: &str, total: Option<u64>, limit: usize) -> Option<usize> {
    let n: usize = input.trim().parse().ok()?;
    if n == 0 {
        return None;
    }
    Some(clamp_page(n, total, limit))
}

/// Plan a `Ctrl+f` / `Ctrl+b` action within the current result page.
pub fn plan_results_page_scroll(
    row: usize,
    row_count: usize,
    visible: usize,
    forward: bool,
    paginated: bool,
    results_page: usize,
    can_next: bool,
) -> ResultsPageScrollPlan {
    if row_count == 0 {
        return ResultsPageScrollPlan::NoPlan;
    }
    let last = row_count - 1;
    let row = row.min(last);
    let step = visible.max(1);
    if forward {
        if row >= last {
            if paginated && can_next {
                ResultsPageScrollPlan::NextPage
            } else {
                ResultsPageScrollPlan::NoPlan
            }
        } else {
            ResultsPageScrollPlan::MoveToRow((row + step).min(last))
        }
    } else if row == 0 {
        if paginated && results_page > 1 {
            ResultsPageScrollPlan::PrevPage
        } else {
            ResultsPageScrollPlan::NoPlan
        }
    } else {
        ResultsPageScrollPlan::MoveToRow(row.saturating_sub(step))
    }
}

struct ToolbarPiece {
    text: String,
    hit: Option<ResultsPaginationHit>,
}

fn toolbar_pieces(
    row_limit: usize,
    page: usize,
    total: Option<u64>,
    row_count: usize,
    counting: bool,
    show_count_button: bool,
) -> Vec<ToolbarPiece> {
    let pages = max_page(total, row_limit);
    let page_label = match pages {
        Some(max) => format!("[p]{page}/{max}"),
        None => format!("[p]{page}"),
    };
    let total_label = match total {
        Some(t) => format!("Total {t} rows"),
        None => format!("Total {row_count} rows"),
    };

    let mut pieces = vec![
        ToolbarPiece {
            text: total_label,
            hit: None,
        },
        ToolbarPiece {
            text: " ".into(),
            hit: None,
        },
    ];

    if show_count_button {
        let count_action = if counting {
            "[c]counting…".to_string()
        } else {
            "[c]count total rows".to_string()
        };
        pieces.push(ToolbarPiece {
            text: count_action,
            hit: Some(ResultsPaginationHit::CountTotalRows),
        });
    }
    pieces.push(ToolbarPiece {
        text: "  ".into(),
        hit: None,
    });
    pieces.extend([
        ToolbarPiece {
            text: format!("{row_limit} [r]rows ▾"),
            hit: Some(ResultsPaginationHit::RowLimit),
        },
        ToolbarPiece {
            text: "  ".into(),
            hit: None,
        },
        ToolbarPiece {
            text: "«".into(),
            hit: Some(ResultsPaginationHit::FirstPage),
        },
        ToolbarPiece {
            text: " ".into(),
            hit: None,
        },
        ToolbarPiece {
            text: "‹".into(),
            hit: Some(ResultsPaginationHit::PrevPage),
        },
        ToolbarPiece {
            text: " ".into(),
            hit: None,
        },
        ToolbarPiece {
            text: page_label,
            hit: Some(ResultsPaginationHit::PageNumber),
        },
        ToolbarPiece {
            text: " ".into(),
            hit: None,
        },
        ToolbarPiece {
            text: "›".into(),
            hit: Some(ResultsPaginationHit::NextPage),
        },
        ToolbarPiece {
            text: " ".into(),
            hit: None,
        },
        ToolbarPiece {
            text: "»".into(),
            hit: Some(ResultsPaginationHit::LastPage),
        },
    ]);
    pieces
}

/// The pagination toolbar line. `accent` / `dim` styles come from the theme.
#[allow(clippy::too_many_arguments)]
pub fn pagination_toolbar_line(
    row_limit: usize,
    page: usize,
    total: Option<u64>,
    row_count: usize,
    counting: bool,
    show_count_button: bool,
    accent: Style,
    dim: Style,
) -> Line<'static> {
    let dim = dim.add_modifier(Modifier::DIM);
    let accent = accent.add_modifier(Modifier::BOLD);

    let spans: Vec<Span<'static>> = toolbar_pieces(
        row_limit,
        page,
        total,
        row_count,
        counting,
        show_count_button,
    )
    .into_iter()
    .map(|piece| match piece.hit {
        Some(ResultsPaginationHit::CountTotalRows) => Span::styled(piece.text, dim),
        Some(ResultsPaginationHit::PageNumber | ResultsPaginationHit::RowLimit) => {
            Span::styled(piece.text, accent)
        }
        Some(_) => Span::styled(piece.text, dim),
        None if piece.text.starts_with("Total ") => Span::styled(piece.text, accent),
        None => Span::raw(piece.text),
    })
    .collect();

    Line::from(spans)
}

/// Lay out clickable regions for the pagination toolbar (right-aligned).
pub fn layout_pagination_bar(
    area: Rect,
    row_limit: usize,
    page: usize,
    total: Option<u64>,
    row_count: usize,
    counting: bool,
    show_count_button: bool,
) -> ResultsPaginationLayout {
    let mut hits = Vec::new();
    if area.width == 0 || area.height == 0 {
        return ResultsPaginationLayout {
            bar_rect: area,
            hits,
        };
    }

    let pieces = toolbar_pieces(
        row_limit,
        page,
        total,
        row_count,
        counting,
        show_count_button,
    );
    let text_len = pieces.iter().map(|p| p.text.chars().count()).sum::<usize>() as u16;
    let bar_width = text_len.min(area.width);
    let bar_x = area.x.saturating_add(area.width.saturating_sub(bar_width));
    let bar_rect = Rect {
        x: bar_x,
        y: area.y,
        width: bar_width,
        height: 1,
    };

    let mut rel_x = 0u16;
    for piece in pieces {
        let width = piece.text.chars().count() as u16;
        if let Some(hit) = piece.hit
            && rel_x < bar_width
        {
            hits.push((
                hit,
                Rect {
                    x: bar_x.saturating_add(rel_x),
                    y: area.y,
                    width: width.min(bar_width.saturating_sub(rel_x)),
                    height: 1,
                },
            ));
        }
        rel_x = rel_x.saturating_add(width);
    }

    ResultsPaginationLayout { bar_rect, hits }
}

/// Resolve the `<` / `>` double-press chord: the first press navigates one
/// page and arms the chord; a second press of the same key within
/// [`RESULTS_PAGE_CHORD_MS`] upgrades to first (`<<`) / last (`>>`) page
/// (original dbm `resolve_page_chord`). A press of the *other* direction (or
/// any expired chord) simply re-arms. Returns `(next_pending, action)`.
pub fn resolve_page_chord(
    pending: Option<(bool, std::time::Instant)>,
    now: std::time::Instant,
    forward: bool,
) -> (
    Option<(bool, std::time::Instant)>,
    Option<ResultsPageAction>,
) {
    match pending {
        Some((dir, at))
            if dir == forward && now.duration_since(at).as_millis() <= RESULTS_PAGE_CHORD_MS =>
        {
            (
                None,
                Some(if forward {
                    ResultsPageAction::Last
                } else {
                    ResultsPageAction::First
                }),
            )
        }
        _ => (
            Some((forward, now)),
            Some(if forward {
                ResultsPageAction::Next
            } else {
                ResultsPageAction::Prev
            }),
        ),
    }
}

/// Position a small popup of `width` × `height` just above `anchor` while
/// staying inside `body` (the pane the toolbar belongs to). Used by the
/// rows-per-page / page-number pickers, which float above their toolbar button
/// instead of covering the whole workspace (matching the original dbm's
/// `layout_page_input_popup` / `layout_row_limit_popup`).
pub fn popup_above_anchor(anchor: Rect, body: Rect, width: u16, height: u16) -> Rect {
    if body.width == 0 || body.height == 0 {
        return Rect::default();
    }
    let width = width.min(body.width);
    // The popup opens upward from the anchor's top edge; if there is not
    // enough room above, the available space caps the height rather than
    // overlapping the button.
    let max_height = anchor.y.saturating_sub(body.y).max(1);
    let height = height.min(max_height);
    let x = anchor
        .x
        .saturating_add(anchor.width / 2)
        .saturating_sub(width / 2)
        .max(body.x)
        .min(body.x.saturating_add(body.width.saturating_sub(width)));
    let y = anchor.y.saturating_sub(height).max(body.y);
    Rect {
        x,
        y,
        width,
        height,
    }
}

pub fn point_in_rect(x: u16, y: u16, rect: Rect) -> bool {
    x >= rect.x
        && y >= rect.y
        && x < rect.x.saturating_add(rect.width)
        && y < rect.y.saturating_add(rect.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_count_rounds_up() {
        assert_eq!(page_count(0, 100), 1);
        assert_eq!(page_count(1, 100), 1);
        assert_eq!(page_count(100, 100), 1);
        assert_eq!(page_count(101, 100), 2);
    }

    #[test]
    fn page_offset_from_one_based_page() {
        assert_eq!(page_offset(1, 100), 0);
        assert_eq!(page_offset(2, 100), 100);
    }

    #[test]
    fn can_go_next_respects_total_cap() {
        assert!(!can_go_next(3, 100, 100, Some(300), false));
        assert!(can_go_next(2, 100, 100, Some(300), false));
    }

    #[test]
    fn can_go_next_at_last_page_blocks() {
        assert!(!can_go_next(10, 100, 100, None, true));
    }

    #[test]
    fn can_go_next_empty_page_blocks_even_with_high_total() {
        assert!(!can_go_next(11, 100, 0, Some(2000), false));
    }

    #[test]
    fn can_go_next_full_last_page_blocks_at_total() {
        assert!(!can_go_next(10, 100, 100, Some(1000), false));
    }

    #[test]
    fn can_go_next_without_total_allows_while_page_full() {
        assert!(can_go_next(1, 100, 100, None, false));
        assert!(!can_go_next(2, 100, 50, None, false));
    }

    #[test]
    fn ctrl_f_moves_by_viewport_then_next_page_at_bottom() {
        assert_eq!(
            plan_results_page_scroll(0, 100, 10, true, true, 1, true),
            ResultsPageScrollPlan::MoveToRow(10)
        );
        assert_eq!(
            plan_results_page_scroll(99, 100, 10, true, true, 1, true),
            ResultsPageScrollPlan::NextPage
        );
        assert_eq!(
            plan_results_page_scroll(99, 100, 10, true, true, 1, false),
            ResultsPageScrollPlan::NoPlan
        );
    }

    #[test]
    fn ctrl_b_moves_by_viewport_then_prev_page_at_top() {
        assert_eq!(
            plan_results_page_scroll(0, 100, 10, false, true, 2, true),
            ResultsPageScrollPlan::PrevPage
        );
        assert_eq!(
            plan_results_page_scroll(0, 100, 10, false, true, 1, true),
            ResultsPageScrollPlan::NoPlan
        );
    }

    #[test]
    fn layout_hits_align_with_toolbar_text() {
        let area = Rect::new(0, 0, 120, 1);
        let layout = layout_pagination_bar(area, 100, 1, None, 100, false, true);
        assert!(!layout.hits.is_empty());
        assert!(layout.bar_rect.width <= area.width);
    }

    #[test]
    fn popup_above_anchor_sits_above_and_inside_body() {
        let body = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        };
        let anchor = Rect {
            x: 100,
            y: 30,
            width: 6,
            height: 1,
        };
        let popup = popup_above_anchor(anchor, body, 24, 5);
        assert_eq!(popup.height, 5);
        assert!(
            popup.bottom() <= anchor.y,
            "popup must sit above the anchor"
        );
        assert!(popup.x >= body.x && popup.right() <= body.right());
        // Horizontally centered over the anchor.
        let anchor_center = anchor.x.saturating_add(anchor.width / 2);
        assert!(popup.x <= anchor_center && popup.right() >= anchor_center);
    }

    #[test]
    fn popup_above_anchor_clamps_height_when_little_room() {
        // Anchor at the very top of the body: only one row is available above,
        // so the popup collapses instead of overlapping the anchor.
        let body = Rect {
            x: 0,
            y: 5,
            width: 80,
            height: 20,
        };
        let anchor = Rect {
            x: 40,
            y: 6,
            width: 6,
            height: 1,
        };
        let popup = popup_above_anchor(anchor, body, 24, 8);
        assert_eq!(popup.y, body.y);
        assert_eq!(popup.height, 1);
        assert!(popup.bottom() <= anchor.y);
    }

    #[test]
    fn first_gt_pages_next_and_arms_the_chord() {
        let t0 = std::time::Instant::now();
        let (pending, action) = resolve_page_chord(None, t0, true);
        assert!(pending.is_some(), "first `>` arms the chord");
        assert_eq!(action, Some(ResultsPageAction::Next));
    }

    #[test]
    fn double_gt_within_window_jumps_last() {
        let t0 = std::time::Instant::now();
        let (pending, _) = resolve_page_chord(None, t0, true);
        let (pending2, action2) = resolve_page_chord(pending, t0, true);
        assert!(pending2.is_none(), "`>>` consumes the chord");
        assert_eq!(action2, Some(ResultsPageAction::Last));
    }

    #[test]
    fn double_lt_within_window_jumps_first() {
        let t0 = std::time::Instant::now();
        let (pending, _) = resolve_page_chord(None, t0, false);
        let (_, action) = resolve_page_chord(pending, t0, false);
        assert_eq!(action, Some(ResultsPageAction::First));
    }

    #[test]
    fn opposite_direction_rearms_instead_of_escalating() {
        let t0 = std::time::Instant::now();
        let (pending, _) = resolve_page_chord(None, t0, true);
        // `>` then `<`: the opposite direction does not escalate; `<` pages
        // back and re-arms in its own direction.
        let (pending2, action2) = resolve_page_chord(pending, t0, false);
        assert_eq!(action2, Some(ResultsPageAction::Prev));
        assert!(pending2.is_some());
        let (_, action3) = resolve_page_chord(pending2, t0, false);
        assert_eq!(action3, Some(ResultsPageAction::First));
    }

    #[test]
    fn expired_chord_pages_once_more() {
        let t0 = std::time::Instant::now();
        let (pending, _) = resolve_page_chord(None, t0, true);
        let late = t0 + std::time::Duration::from_millis(RESULTS_PAGE_CHORD_MS as u64 + 1);
        let (pending2, action) = resolve_page_chord(pending, late, true);
        assert_eq!(
            action,
            Some(ResultsPageAction::Next),
            "late repeat is a plain page"
        );
        assert!(pending2.is_some());
    }
}
