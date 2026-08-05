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

    let pieces = toolbar_pieces(row_limit, page, total, row_count, counting, show_count_button);
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
}
