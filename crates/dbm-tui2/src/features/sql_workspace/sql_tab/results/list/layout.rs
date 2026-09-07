//! Results list geometry: header/body regions, cell hit-testing and
//! viewport scroll math.

use super::state::ListState;
use crate::common::layout::pane_scrollbar::{PaneScrollLayout, pane_scroll_layout};
use crate::common::view::action_bar::{
    RESULTS_ACTION_BAR_HEIGHT, ResultsAction, ResultsToolbarModel, action_bar_width,
    layout_action_bar,
};
use crate::common::view::format::{RESULTS_HEADER_HEIGHT, RESULTS_ROW_HEIGHT};
use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Split a list region (the content band narrowed to the list side) vertically
/// into the action bar (top) and the table body. Single source of truth used by
/// both the renderer and the hit-test paths.
pub fn results_list_regions(list_area: Rect) -> (Rect, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(RESULTS_ACTION_BAR_HEIGHT),
            Constraint::Min(0),
        ])
        .split(list_area);
    (chunks[0], chunks[1])
}

/// The enable/disable model for the Results action bar, derived from the list
/// state. Single source of truth shared by the renderer (`list::view`) and the
/// click hit-testing (`action_bar_button_at`) so they can never disagree about
/// which buttons are active.
pub fn results_toolbar_model(state: &ListState) -> ResultsToolbarModel {
    let has_result = state.result.is_some();
    let editable = state.editable();
    let commit_n = state.commit_row_count();
    let edit_active = state.edit.editing;
    let edit_dirty = state.edit.is_dirty();
    ResultsToolbarModel {
        refresh_enabled: has_result,
        edit_enabled: editable,
        edit_active,
        commit_enabled: edit_active && commit_n > 0,
        rollback_enabled: edit_active && edit_dirty,
        commit_n,
        edit_reason: state.edit_blocked_reason.clone(),
    }
}

/// The action-bar button the pointer `(x, y)` lands on, if the button is
/// enabled. Mirrors exactly the geometry `list::view` draws — `results_list_regions`
/// with the h_scroll-clamped `layout_action_bar` — so clicking always hits what
/// is painted.
pub fn action_bar_button_at(
    list_area: Rect,
    state: &ListState,
    x: u16,
    y: u16,
) -> Option<ResultsAction> {
    let (bar_area, _) = results_list_regions(list_area);
    if bar_area.width == 0 || !contains(bar_area, x, y) {
        return None;
    }
    let model = results_toolbar_model(state);
    let bar_scroll_max = action_bar_width(&model).saturating_sub(bar_area.width);
    let bar_scroll = (state.h_scroll.get() as u16).min(bar_scroll_max);
    for (action, rect, enabled) in layout_action_bar(bar_area, &model, bar_scroll) {
        if enabled && contains(rect, x, y) {
            return Some(action);
        }
    }
    None
}

/// Column index whose header boundary the pointer is over in the results
/// table, if any. Mirrors the original dbm's `resize_hit_column`: only the
/// top header lines (`RESULTS_HEADER_CONTENT_HEIGHT`) count, and the pointer
/// must be within one column of a column's right edge. Used for the splitter
/// hover indicator and to begin a column-width drag.
pub fn col_resize_hit_at(list_area: Rect, state: &ListState, x: u16, y: u16) -> Option<usize> {
    let (_table_area, content_area, h_scroll) = results_geometry(list_area, state)?;
    if !contains(content_area, x, y) {
        return None;
    }
    let rel_y = y.saturating_sub(content_area.y);
    let rel_x = x.saturating_sub(content_area.x) as usize;
    crate::common::view::format::resize_hit_column(rel_x, rel_y, h_scroll as u16, &state.col_widths)
}

/// Hit-test a click at `(x, y)` inside the list's **inner** area (already
/// inside the outer Results Block). Returns `Some((row, col))` when the
/// click lands on a data cell (not header, action bar, pagination, footer,
/// or scrollbar), or `None` otherwise.
///
/// Uses [`results_list_regions`] so hit-test geometry always matches the
/// renderer's split logic exactly.
pub fn cell_hit_at(list_area: Rect, state: &ListState, x: u16, y: u16) -> Option<(usize, usize)> {
    if list_area.width == 0 || list_area.height == 0 {
        return None;
    }

    let result = state.result.as_ref()?;
    if result.columns.is_empty() || result.rows.is_empty() {
        return None;
    }

    let col_widths = &state.col_widths;

    // Use the single source of truth for the list geometry.
    let (_, table_area) = results_list_regions(list_area);

    let row_count = result.rows.len();
    let table_width = crate::common::view::format::results_table_width(col_widths);

    // ---- SHARED VIEWPORT CALCULATION ----
    // Use the exact same compute_viewport_scroll that render_table uses —
    // no more duplicated anchor logic that silently drifts from render.
    let vs = compute_viewport_scroll(
        table_area,
        state,
        row_count,
        col_widths,
        table_width as usize,
    )?;

    if !contains(vs.layout.content_area, x, y) {
        return None;
    }

    // Hit-test row: y relative to content_area, minus header.
    let rel_y = y.saturating_sub(vs.layout.content_area.y);
    if rel_y < RESULTS_HEADER_HEIGHT {
        return None;
    }
    let rel_data_y = rel_y.saturating_sub(RESULTS_HEADER_HEIGHT);
    let row_in_viewport = rel_data_y / RESULTS_ROW_HEIGHT;
    let row_idx = vs.v_scroll + usize::from(row_in_viewport);
    if row_idx >= row_count {
        return None;
    }

    // Hit-test column: x relative to content_area, accounting for h_scroll.
    let rel_x = x.saturating_sub(vs.layout.content_area.x) as usize;
    let col_sx = rel_x.saturating_add(vs.h_scroll);
    for col in 0..result.columns.len() {
        let start = crate::common::view::format::col_x_start(col, col_widths);
        let end = crate::common::view::format::col_x_end(col, col_widths);
        if col_sx >= start && col_sx < end {
            return Some((row_idx, col));
        }
    }

    None
}

/// Resolve the shared results-list geometry used by both this hit-test and the
/// drag width mapping: `(table_area, content_area, h_scroll)`. `table_area` is
/// the table body rect via [`results_list_regions`]; `content_area` and
/// `h_scroll` come from [`compute_viewport_scroll`], the same source of truth
/// the renderer uses. Returns `None` when there is no table to resize.
pub(super) fn results_geometry(list_area: Rect, state: &ListState) -> Option<(Rect, Rect, usize)> {
    if list_area.width == 0 || list_area.height == 0 || state.result.is_none() {
        return None;
    }
    let result = state.result.as_ref()?;
    if result.columns.is_empty() || result.rows.is_empty() {
        return None;
    }
    let col_widths = &state.col_widths;
    let (_, table_area) = results_list_regions(list_area);
    let table_width = crate::common::view::format::results_table_width(col_widths);
    let vs = compute_viewport_scroll(
        table_area,
        state,
        result.rows.len(),
        col_widths,
        table_width as usize,
    )?;
    Some((table_area, vs.layout.content_area, vs.h_scroll))
}

pub(super) fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x.saturating_add(r.width) && y >= r.y && y < r.y.saturating_add(r.height)
}

/// Compute the visible-area viewport and anchored scrolls for the given
/// Results table `area` and `state`. Mirrors discover-style scrolling: when
/// the cursor lies within the viewport we keep v_scroll where it is; when
/// it leaves we push the viewport to track.
///
/// When `scroll_locked` is set (scrollbar drag / manual SetVScroll / SetHScroll)
/// both anchors are skipped so the manually-set scroll is honoured.
pub fn compute_viewport_scroll(
    area: Rect,
    state: &ListState,
    row_count: usize,
    col_widths: &[u16],
    table_width: usize,
) -> Option<ViewportScroll> {
    if area.width == 0 || area.height == 0 || row_count == 0 {
        return None;
    }

    let layout = pane_scroll_layout(area, table_width as u16, row_count, area.height as usize);
    let content_area = layout.content_area;
    if content_area.height < RESULTS_HEADER_HEIGHT {
        return None;
    }

    let visible_data_rows =
        usize::from(content_area.height.saturating_sub(RESULTS_HEADER_HEIGHT) / RESULTS_ROW_HEIGHT);
    let vr = visible_data_rows.max(1);
    let max_v_scroll = row_count.saturating_sub(vr);
    let max_h_scroll =
        crate::common::view::format::results_max_h_scroll(table_width as u16, content_area.width)
            as usize;

    let scroll_locked = state.scroll_locked.get();

    // ---- h_scroll anchor (pixel-column based) ----
    let viewport_w = content_area.width as usize;
    let mut h_scroll = state.h_scroll.get().min(max_h_scroll);
    if !scroll_locked && viewport_w > 0 && !col_widths.is_empty() {
        let view_right = h_scroll.saturating_add(viewport_w);
        let cur_col_left = crate::common::view::format::col_x_start(state.col, col_widths);
        let cur_col_right = crate::common::view::format::col_x_end(state.col, col_widths);
        if cur_col_right <= h_scroll {
            h_scroll = cur_col_left;
        } else if cur_col_left >= view_right {
            h_scroll = cur_col_right.saturating_sub(viewport_w);
        }
        h_scroll = h_scroll.min(max_h_scroll);
    }

    // ---- v_scroll anchor (data-row based) ----
    let mut v_scroll = state.v_scroll.get().min(max_v_scroll);
    if !scroll_locked {
        if state.row >= v_scroll + vr {
            v_scroll = state.row.saturating_sub(vr.saturating_sub(1));
        } else if state.row < v_scroll {
            v_scroll = state.row;
        }
    }
    v_scroll = v_scroll.min(max_v_scroll);

    Some(ViewportScroll {
        layout,
        visible_data_rows,
        v_scroll,
        h_scroll,
        max_v_scroll,
        max_h_scroll,
    })
}

/// Result of [`compute_viewport_scroll`]: the shared viewport calculation
/// used by both [`render_table`] and [`cell_hit_at`]. Keeping both paths
/// honest to the same formula eliminates cursor-anchor drift between the
/// drawn rows and the click-resolved row.
pub struct ViewportScroll {
    /// Full pane_scroll_layout result — has content_area and both scrollbar rects.
    pub layout: PaneScrollLayout,
    /// Number of data rows visible inside `content_area`.
    pub visible_data_rows: usize,
    /// Anchored v_scroll (data-row index of first visible row).
    pub v_scroll: usize,
    /// Anchored h_scroll (pixel offset into the table width).
    pub h_scroll: usize,
    /// Maximum valid v_scroll value.
    pub max_v_scroll: usize,
    /// Maximum valid h_scroll value.
    pub max_h_scroll: usize,
}
