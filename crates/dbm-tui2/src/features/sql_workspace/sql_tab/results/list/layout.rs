//! Results list geometry: header/body regions, cell hit-testing and
//! viewport scroll math.

use super::state::ListState;
use crate::common::layout::pane_scrollbar::{PaneScrollLayout, pane_scroll_layout};
use crate::common::view::action_bar::{
    ResultsAction, ResultsToolbarModel, action_bar_height, layout_action_bar,
};
use crate::common::view::format::{RESULTS_HEADER_HEIGHT, RESULTS_ROW_HEIGHT};
use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// How many rows the Results action bar needs for `list_area`'s width: buttons
/// wrap onto extra rows instead of scrolling when the pane is narrow. Single
/// source of truth for both the renderer and the hit-test paths.
pub fn results_action_rows(list_area: Rect, state: &ListState) -> u16 {
    let model = results_toolbar_model(state);
    action_bar_height(&model, list_area.width)
}

/// Split a list region (the content band narrowed to the list side) vertically
/// into the action bar (top, [`results_action_rows`] tall) and the table body.
/// Single source of truth used by both the renderer and the hit-test paths.
pub fn results_list_regions(list_area: Rect, action_rows: u16) -> (Rect, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(action_rows.max(1)), Constraint::Min(0)])
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
        // Refresh greys out during its 1s cooldown (mirroring the original dbm).
        refresh_enabled: has_result && state.refresh_allowed(),
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
    let rows = results_action_rows(list_area, state);
    let (bar_area, _) = results_list_regions(list_area, rows);
    if bar_area.width == 0 || !contains(bar_area, x, y) {
        return None;
    }
    let model = results_toolbar_model(state);
    for (action, rect, enabled) in layout_action_bar(bar_area, &model, 0) {
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
/// Width of the pinned left change-marker gutter: one column while an edit
/// session is active, otherwise none.
///
/// The renderer applies this as a constant offset when turning a column's
/// column-space position into a screen x, and every hit-test subtracts it
/// again — one shared helper so clicks can never drift from the drawn grid.
pub fn results_gutter_width(state: &ListState) -> u16 {
    if state.edit.editing {
        crate::common::view::format::RESULTS_DIRTY_GUTTER_WIDTH
    } else {
        0
    }
}

pub fn col_resize_hit_at(list_area: Rect, state: &ListState, x: u16, y: u16) -> Option<usize> {
    let (_table_area, content_area, h_scroll) = results_geometry(list_area, state)?;
    if !contains(content_area, x, y) {
        return None;
    }
    let gutter_w = results_gutter_width(state);
    let rel_y = y.saturating_sub(content_area.y);
    let rel_x = x.saturating_sub(content_area.x).saturating_sub(gutter_w) as usize;
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
    let (_, table_area) = results_list_regions(list_area, results_action_rows(list_area, state));

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

    // Hit-test column: x relative to content_area (minus the edit gutter),
    // accounting for h_scroll.
    let gutter_w = results_gutter_width(state);
    let rel_x = x
        .saturating_sub(vs.layout.content_area.x)
        .saturating_sub(gutter_w) as usize;
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
    let (_, table_area) = results_list_regions(list_area, results_action_rows(list_area, state));
    let table_width = crate::common::view::format::results_table_width(col_widths)
        .saturating_add(results_gutter_width(state));
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
        // Column positions live in content space, which starts *after* the edit
        // gutter — so the anchor compares against gutter-shifted edges.
        let gutter = results_gutter_width(state) as usize;
        let cur_col_left = gutter + crate::common::view::format::col_x_start(state.col, col_widths);
        let cur_col_right = gutter + crate::common::view::format::col_x_end(state.col, col_widths);
        if cur_col_right <= h_scroll {
            h_scroll = cur_col_left;
        } else if cur_col_left >= view_right {
            h_scroll = cur_col_right.saturating_sub(viewport_w);
        } else if cur_col_right > view_right {
            // The selected column's left edge is already inside the viewport but
            // its right edge extends past it (a trailing column peeking into the
            // view). Scroll it fully into view, right-aligned. Without this a
            // cursor on the last column could never reveal the column's
            // truncated right side: there is no next column to move to, so no
            // later anchor would ever push the horizontal scrollbar further.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::view::format::{RESULTS_HEADER_HEIGHT, RESULTS_ROW_HEIGHT};
    use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;
    use crate::features::sql_workspace::sql_tab::results::state::QueryResultData;

    fn state(editing: bool) -> ListState {
        let mut s = ListState::new();
        s.result = Some(QueryResultData {
            columns: vec![
                ColumnInfo {
                    name: "id".into(),
                    type_name: "int4".into(),
                    type_display: "int4".into(),
                    comment: None,
                },
                ColumnInfo {
                    name: "name".into(),
                    type_name: "text".into(),
                    type_display: "text".into(),
                    comment: None,
                },
            ],
            rows: vec![vec!["1".into(), "alice".into()]],
            rows_affected: None,
            total_rows: Some(1),
        });
        s.col_widths = vec![10, 10];
        s.selected = true;
        s.scroll_locked.set(true);
        s.h_scroll.set(0);
        if editing {
            s.edit.editing = true;
        }
        s
    }

    #[test]
    fn gutter_only_exists_while_editing() {
        assert_eq!(results_gutter_width(&state(false)), 0);
        assert_eq!(
            results_gutter_width(&state(true)),
            crate::common::view::format::RESULTS_DIRTY_GUTTER_WIDTH
        );
    }

    #[test]
    fn editing_gutter_shifts_cell_hits_with_the_drawn_grid() {
        // A click one pixel right of the content start must resolve to column 0
        // while editing (the first pixel is the change-marker gutter) and to
        // column 0 one pixel earlier when not editing. Both must agree with the
        // renderer's offset, or clicks land on the wrong cell. First data row
        // sits at `content.y + RESULTS_HEADER_HEIGHT` (header included in the
        // content band). `rel_y` is measured from `content.y`.
        let area = Rect::new(0, 0, 40, 12);
        let first_row_y =
            results_geometry(area, &state(true)).expect("geometry").1.y + RESULTS_HEADER_HEIGHT;

        let plain = state(false);
        let editing_state = state(true);
        let (_t, content_plain, _h) = results_geometry(area, &plain).expect("geometry");
        let (_t, content_edit, _h) = results_geometry(area, &editing_state).expect("geometry");

        // Column 0 occupies [0, 10) in column space. Without the gutter its
        // first pixel is the content start; with the gutter its first *content*
        // pixel sits one pixel later (the marker is drawn at the content start).
        assert_eq!(
            cell_hit_at(area, &plain, content_plain.x, first_row_y),
            Some((0, 0)),
            "no gutter: content start is column 0"
        );
        // The pixel that is column 0's *last* cell pixel in the plain grid is
        // still column 0 while editing (the whole grid shifted right by 1), and
        // the gutter pixel itself maps back to column 0 (1-px tolerance to the
        // neighbouring cell, as format::results_cell_at_point also does).
        let last_col0_px = 9;
        assert_eq!(
            cell_hit_at(
                area,
                &editing_state,
                content_edit.x + last_col0_px,
                first_row_y
            ),
            Some((0, 0)),
            "editing: column 0 spans [gutter, gutter+width)"
        );
        // 10 plain pixels right of the content start is column 1's first pixel
        // when NOT editing, but still column 0 (width-1) when editing — proving
        // the hit test follows the renderer's gutter shift.
        let x = 10;
        assert_eq!(
            cell_hit_at(area, &plain, content_plain.x + x, first_row_y),
            Some((0, 1))
        );
        assert_eq!(
            cell_hit_at(area, &editing_state, content_edit.x + x, first_row_y),
            Some((0, 0)),
            "editing shifts the grid so pixel 10 is still inside column 0"
        );
    }

    #[test]
    fn column_resize_hit_tracks_the_gutter() {
        let area = Rect::new(0, 0, 40, 12);
        let s = state(true);
        let (t, content, _h) = results_geometry(area, &s).expect("geometry");
        // Column 0's right border, screen x = content.x + gutter + width - 1,
        // must be found as a resize handle on one of the header lines.
        let width = s.col_widths[0];
        let edge = content.x + results_gutter_width(&s) + width;
        let header_y = content.y; // first header content line (rel_y=0)
        let _ = t;
        assert_eq!(col_resize_hit_at(area, &s, edge - 1, header_y), Some(0));
    }

    #[test]
    fn row_hit_still_uses_row_height() {
        let area = Rect::new(0, 0, 40, 12);
        let s = state(true);
        let (t, content, _h) = results_geometry(area, &s).expect("geometry");
        let _ = t;
        let first_row_y = content.y + RESULTS_HEADER_HEIGHT;
        // A click one row-height below the first row slot is empty (only one row).
        assert_eq!(
            cell_hit_at(
                area,
                &s,
                content.x + results_gutter_width(&s),
                first_row_y + RESULTS_ROW_HEIGHT,
            ),
            None,
            "a single row of data: the second row slot is empty"
        );
    }

    #[test]
    fn anchor_brings_a_right_peeking_trailing_column_fully_into_view() {
        // A trailing column whose left edge is already visible but whose right
        // edge extends past the viewport can never be fully revealed by moving
        // the cursor further right (there is no next column). The anchor must
        // pull it right-aligned into view the moment it is selected.
        let area = Rect::new(0, 0, 20, 12);
        let mut s = ListState::new();
        s.result = Some(QueryResultData {
            columns: vec![ColumnInfo {
                name: "wide".into(),
                type_name: "int4".into(),
                type_display: "int4".into(),
                comment: None,
            }],
            rows: vec![vec!["x".into()]; 60],
            rows_affected: None,
            total_rows: Some(60),
        });
        s.col_widths = vec![30];
        s.col = 0;
        s.h_scroll.set(0);
        s.scroll_locked.set(false);

        // Column [0, 30) with a viewport narrower than 30: the column peeks in
        // from the left edge and its right edge is cut off. Selecting it must
        // scroll so the column's right edge sits at the viewport's right edge.
        let vs = compute_viewport_scroll(area, &s, 60, &s.col_widths, 30)
            .expect("a viewport with overflow");
        let content_w = vs.layout.content_area.width.max(1) as usize;
        assert!(content_w < 30, "setup: the column must exceed the viewport");
        assert_eq!(
            vs.h_scroll,
            30 - content_w,
            "a right-peeking trailing column must be scrolled fully into view"
        );

        // Manual scroll (locked) still honours the user's chosen position.
        s.scroll_locked.set(true);
        s.h_scroll.set(0);
        let vs = compute_viewport_scroll(area, &s, 60, &s.col_widths, 30)
            .expect("a viewport with overflow");
        assert_eq!(vs.h_scroll, 0, "scroll_locked keeps the manual position");
    }
}
