//! Results list sub-module rendering: the action bar, the result table,
//! the pagination toolbar, and the list footer.
//!
//! The outer Block with border + title is drawn by the parent
//! `super::render()` — this module renders borderless content into the
//! already-inner area.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::common::view::action_bar::{
    RESULTS_ACTION_BAR_HEIGHT, ResultsToolbarModel, action_bar_width, draw_action_bar,
};
use crate::common::view::format::{
    RESULTS_HEADER_HEIGHT, RESULTS_ROW_CONTENT_HEIGHT, RESULTS_ROW_HEIGHT,
    column_type_label, results_col_text_view,
};
use crate::common::view::hints::{draw_pane_footer, footer_height};
use crate::common::view::theme::Theme;
use crate::common::view::pane_scrollbar::{PaneScrollLayout, pane_scroll_layout};

use super::state::ListState;
use super::super::pagination::{RESULTS_PAGINATION_BAR_HEIGHT, pagination_toolbar_line};

/// Render the list sub-feature: action bar + table + pagination + footer,
/// borderless — the outer Block is drawn by the parent `results::render()`.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ListState,
    focused: bool,
    detail_open: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = theme.palette();

    // A failed query is shown as red error text.
    if let Some(message) = state.query_error.as_deref() {
        render_error(frame, theme, area, message);
        return;
    }

    let Some(result) = state.result.as_ref() else {
        render_empty(frame, theme, area);
        return;
    };

    let total_rows = result.total_rows;
    let row_count = state.row_count();
    let state_h_scroll = state.h_scroll.get();

    // `area` is already the inner area of the outer Results Block.
    let search_active = state.search.text_input_active();
    let sql_status = state.executed_sql_display();
    let (table_body, action_bar_area, pagination_area, footer_area) =
        compute_table_area(area, row_count, search_active, detail_open, &sql_status);

    let hint = crate::common::view::hints::results_pane_footer_text(
        search_active,
        detail_open,
        &sql_status,
    );

    // Content area: action bar + table.
    let has_result = state.result.is_some();
    let editable = state.editable();
    let commit_n = state.commit_row_count();
    let edit_active = state.edit.editing;
    let edit_dirty = state.edit.is_dirty();
    let edit_reason = state.edit_blocked_reason.clone();
    let row_limit = state.row_limit;
    let page = state.page;
    let (bar_scroll_max, model) = {
        let model = ResultsToolbarModel {
            refresh_enabled: has_result,
            edit_enabled: editable,
            edit_active,
            commit_enabled: edit_active && commit_n > 0,
            rollback_enabled: edit_active && edit_dirty,
            commit_n,
            edit_reason,
        };
        (
            action_bar_width(&model).saturating_sub(action_bar_area.width),
            model,
        )
    };
    let bar_scroll = state_h_scroll.min(bar_scroll_max as usize) as u16;

    draw_action_bar(frame, action_bar_area, &model, bar_scroll, p);
    render_table(
        frame,
        theme,
        table_body,
        state,
        focused,
    );

    // Pagination toolbar (full width, below the content area, inside the Block).
    if let Some(pag_area) = pagination_area {
        let toolbar = pagination_toolbar_line(
            row_limit,
            page,
            total_rows,
            row_count,
            false,
            false,
            Style::default().fg(p.accent),
            Style::default().fg(p.muted),
        );
        frame.render_widget(Paragraph::new(toolbar), pag_area);
    }

    // Results list footer (full width, inside the Block).
    draw_pane_footer(frame, theme, footer_area, &hint);
}

/// Single source of truth for how the results list's already-inner area is
/// split into action_bar + table_body + (pagination) + footer. Both [`render`]
/// and the hit-test path in [`crate::features::sql_workspace::sql_tab::view`]
/// must call this.
///
/// Returns `(table_body_area, action_bar_area, pagination_area, footer_area)`.
/// `pagination_area` is `None` when there are no rows to paginate.
pub fn compute_table_area(
    list_inner: Rect,
    row_count: usize,
    search_active: bool,
    detail_open: bool,
    sql_status: &str,
) -> (Rect, Rect, Option<Rect>, Rect) {
    let hint = crate::common::view::hints::results_pane_footer_text(
        search_active,
        detail_open,
        sql_status,
    );
    let footer_h = footer_height(&hint, list_inner.width).min(list_inner.height.saturating_sub(4));

    let pagination_h = if row_count > 0 { RESULTS_PAGINATION_BAR_HEIGHT } else { 0 };

    let chunks = if pagination_h > 0 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(pagination_h),
                Constraint::Length(footer_h),
            ])
            .split(list_inner)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(footer_h),
            ])
            .split(list_inner)
    };

    let content = chunks[0];
    let pagination_area = if pagination_h > 0 { Some(chunks[1]) } else { None };
    let footer_area = if pagination_h > 0 { chunks[2] } else { chunks[1] };

    let list_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(RESULTS_ACTION_BAR_HEIGHT),
            Constraint::Min(0),
        ])
        .split(content);

    let action_bar_area = list_chunks[0];
    let table_body_area = list_chunks[1];

    (table_body_area, action_bar_area, pagination_area, footer_area)
}

fn render_error(frame: &mut Frame, theme: &Theme, area: Rect, message: &str) {
    let p = theme.palette();
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            message,
            Style::default().fg(p.error),
        ))),
        area,
    );
}

fn render_empty(frame: &mut Frame, theme: &Theme, area: Rect) {
    let p = theme.palette();
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Run a query to see results",
            Style::default().fg(p.muted),
        ))),
        area,
    );
}

/// Compute the per-cell text skip (display cells) that keeps the current
/// match's `query` visible inside the match cell. Derived at render time — it
/// only has meaning for the cell holding the current match, so callers must
/// apply it only to that cell and never store it as global state.
///
/// `table_skip` is the column's existing horizontal skip (`tv.table_text_skip`);
/// the returned value cancels it so the match lands at `start - 1` in the cell
/// view, mirroring the original dbm.
fn match_cell_text_skip(
    value: &str,
    m: super::search::ResultsSearchMatch,
    query: &str,
    text_w: u16,
    table_skip: u16,
) -> u16 {
    let visible = text_w.saturating_sub(1);
    if visible == 0 || query.is_empty() {
        return 0;
    }
    let match_start_w = crate::common::view::format::display_width_char_prefix(value, m.start);
    let query_w = crate::common::view::format::cell_display_width(query);
    let max_skip = crate::common::view::format::max_cell_text_skip(value, visible);
    let ts = table_skip as usize;

    let mut cell_skip = match_start_w.saturating_sub(ts).saturating_sub(1);
    if match_start_w + query_w > cell_skip + ts + visible as usize {
        cell_skip = match_start_w
            .saturating_add(query_w)
            .saturating_sub(ts)
            .saturating_sub(visible as usize);
    }
    (cell_skip as u16).min(max_skip)
}

/// The frame around the current-match cell is drawn with the palette's match
/// border emphasis (accent) so it stands out from the muted grid.
fn current_match_border_style(p: &crate::common::view::theme::Palette) -> Style {
    p.match_cell_border_style()
}

/// Recolour the grid border around the current-match cell into an accent
/// frame: the top `─` (with corners) and the left/right `│`. The bottom edge
/// coincides with the row separator, which is tinted separately via the
/// returned `(left, right)` span. Skips cells that touch the pane edge where
/// there is no interior border to recolor.
fn draw_match_cell_frame(
    frame: &mut Frame,
    style: Style,
    table_area: Rect,
    content_x: u16,
    content_y: u16,
    border_x: u16,
) -> Option<(u16, u16)> {
    let left = content_x.saturating_sub(1);
    let right = border_x;
    let top = content_y.saturating_sub(1);
    let bottom = content_y.saturating_add(RESULTS_ROW_CONTENT_HEIGHT);
    if right <= left
        || left < table_area.x
        || right >= table_area.right()
        || bottom > table_area.bottom()
    {
        return None;
    }
    // Top edge + corners; bottom edge is the row separator (tinted by caller).
    frame.buffer_mut().set_string(left, top, "┌", style);
    for x in (left + 1)..right {
        frame.buffer_mut().set_string(x, top, "─", style);
    }
    frame.buffer_mut().set_string(right, top, "┐", style);
    // Left/right edges spanning the content row down to the separator line.
    for y in (top + 1)..=bottom {
        frame.buffer_mut().set_string(left, y, "│", style);
        frame.buffer_mut().set_string(right, y, "│", style);
    }
    Some((left, right))
}

/// Render the result table body directly into `area` (no own Block/borders).
/// The outer Block with title is created by the caller (`render`).
///
/// Layout (matching original dbm):
///   Header: 2 lines (name + type label) + 1 separator = 3 rows total
///   Each data row: 1 content line + 1 separator = 2 rows total
///   Column borders: │ character between columns
///
/// After computing the auto-adjusted h_scroll / v_scroll that keeps the cursor
/// anchored inside the viewport, the values are synced back into `state` so
/// the next frame starts from the correct scroll position (fixes the stale
/// h_scroll problem where state.h_scroll was never updated from the view).
fn render_table(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ListState,
    _focused: bool,
) {
    // Extract all needed values first to avoid borrow conflicts.
    let result = match state.result.as_ref() {
        Some(r) => r,
        None => return,
    };
    let col_widths = &state.col_widths;
    let state_row = state.row;
    let state_col = state.col;
    let state_selected = state.selected;
    let p = theme.palette();

    // Search highlight inputs: derived once so the cell loop stays flat.
    let search_query = if state.search.query.trim().is_empty() {
        None
    } else {
        Some(state.search.query.as_str())
    };
    let search_matches = &state.search_matches;
    let current_match = state.search_matches.get(state.search_match_index).copied();

    if result.columns.is_empty() {
        let affected = result.rows_affected;
        let text = match affected {
            Some(n) => format!("{n} rows affected"),
            None => "Query completed".to_string(),
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                text,
                Style::default().fg(p.fg),
            ))),
            area,
        );
        return;
    }

    let row_count = result.rows.len();
    let num_cols = result.columns.len();

    // Compute actual table content width to detect horizontal overflow.
    let table_width = crate::common::view::format::results_table_width(col_widths);

    // ---- SHARED VIEWPORT CALCULATION ----
    // One source of truth: both render and cell_hit_at call this exact
    // function so the anchored v_scroll / h_scroll / visible_data_rows
    // always match between what we draw and what we hit-test.
    let vs = match compute_viewport_scroll(area, state, row_count, col_widths, table_width as usize) {
        Some(v) => v,
        None => return,
    };

    let layout = &vs.layout;
    let table_area = layout.content_area;

    // Visible content width: when columns don't fill the viewport, avoid
    // rendering empty space beyond the last column (matching original dbm).
    let content_width = table_width.min(table_area.width);

    let h_scroll = vs.h_scroll as u16;
    let v_scroll = vs.v_scroll;
    let visible_data_rows = vs.visible_data_rows;

    // Row separator style (subtle grid line).
    let grid_style = Style::default().fg(p.muted);

    // ---- HEADER (3 lines) ----
    // Line 0: column names (bold)
    // Line 1: type labels (green)
    // Line 2: separator
    for col in 0..num_cols {
        let Some(meta) = result.columns.get(col) else { break };
        let Some(tv) = results_col_text_view(col, col_widths, table_area.width, h_scroll) else {
            continue;
        };
        if tv.text_w == 0 {
            continue;
        }

        // Column's screen x = text_vis_left − h_scroll (matching original dbm).
        let col_x = table_area
            .x
            .saturating_add(
                crate::common::view::format::col_x_start(col, col_widths) as u16
                    + tv.table_text_skip,
            )
            .saturating_sub(h_scroll);

        // Column name (bold).
        let name_style = if col == state_col && state_selected {
            Style::default()
                .fg(p.selection_text)
                .bg(p.selection_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg).add_modifier(Modifier::BOLD)
        };
        let name = crate::common::view::format::truncate_cell_display_from(
            &meta.name,
            tv.table_text_skip,
            tv.text_w,
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(name, name_style))),
            Rect::new(col_x, table_area.y, tv.text_w, 1),
        );

        // Type label: dark grey on selection bg for hierarchy, green otherwise.
        let type_label = column_type_label(meta);
        let type_style = if col == state_col && state_selected {
            Style::default().fg(p.selection_text).bg(p.selection_bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        };
        let type_text = crate::common::view::format::truncate_cell_display_from(
            &type_label,
            tv.table_text_skip,
            tv.text_w,
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(type_text, type_style))),
            Rect::new(col_x, table_area.y + 1, tv.text_w, 1),
        );

        // Column border (│) at the right edge of this column.
        let col_right = crate::common::view::format::col_x_end(col, col_widths);
        let border_x = table_area
            .x
            .saturating_add(col_right as u16)
            .saturating_sub(h_scroll)
            .saturating_sub(1);
        if border_x >= table_area.x && border_x < table_area.x + table_area.width {
            for y in table_area.y..(table_area.y + RESULTS_HEADER_HEIGHT).min(table_area.bottom()) {
                frame
                    .buffer_mut()
                    .set_string(border_x, y, "│", grid_style);
            }
        }
    }

    // Horizontal separator after header.
    let sep_y = table_area.y + RESULTS_HEADER_HEIGHT - 1;
    if sep_y < table_area.bottom() {
        let sep_right = table_area.x.saturating_add(content_width);
        for x in table_area.x..sep_right.min(table_area.right()) {
            frame.buffer_mut().set_string(x, sep_y, "─", grid_style);
        }
    }

    // ---- DATA ROWS ----
    for vis in 0..visible_data_rows {
        let row_idx = v_scroll + vis;
        if row_idx >= row_count {
            break;
        }
        let y_base = table_area
            .y
            .saturating_add(RESULTS_HEADER_HEIGHT)
            .saturating_add(vis as u16 * RESULTS_ROW_HEIGHT);

        // Row content area — only spans the actual column content width
        // (not the full viewport) to avoid empty-column appearance.
        let row_content_area = Rect::new(
            table_area.x,
            y_base,
            content_width,
            RESULTS_ROW_CONTENT_HEIGHT,
        );

        // Row background (highlight if selected).
        let row_selected = row_idx == state_row && state_selected;
        if row_selected {
            frame.render_widget(
                ratatui::widgets::Block::default().style(Style::default().bg(p.selection_bg)),
                row_content_area,
            );
        }

        // Horizontal span (`(left, right)`) of the current-match cell on this
        // row, if any; the row separator below tints this segment as the cell's
        // bottom border so all four edges are drawn consistently.
        let mut match_bottom_span: Option<(u16, u16)> = None;

        // Draw each cell.
        for col in 0..num_cols {
            let Some(tv) = results_col_text_view(col, col_widths, table_area.width, h_scroll) else {
                continue;
            };
            if tv.text_w == 0 {
                continue;
            }

            let value = result
                .rows
                .get(row_idx)
                .and_then(|r| r.get(col))
                .map(String::as_str)
                .unwrap_or("");

            let is_active = state_col == col && state_selected;
            let cell_selected = row_selected && is_active;
            let base_style = if cell_selected {
                Style::default()
                    .fg(p.selection_focus_text)
                    .bg(p.selection_cell_bg)
                    .add_modifier(Modifier::BOLD)
            } else if row_selected || is_active {
                Style::default().fg(p.selection_text).bg(p.selection_bg)
            } else {
                Style::default().fg(p.fg)
            };

            // Shift the window for the cell holding the current match so the highlighted
            // query stays in view — including when the focus has moved to another
            // cell, so the viewport stays aligned with the offset/length read-out
            // (which still describes the current match). It is derived at render
            // time and never cached (stale skips on other cells are impossible).
            let is_match_cell = current_match.is_some_and(|m| m.row == row_idx && m.col == col);
            let text_skip = if is_match_cell {
                let m = current_match.unwrap();
                let q = search_query.unwrap_or("");
                tv.table_text_skip
                    .saturating_add(match_cell_text_skip(value, m, q, tv.text_w, tv.table_text_skip))
            } else {
                tv.table_text_skip
            };
            let col_x = table_area
                .x
                .saturating_add(
                    crate::common::view::format::col_x_start(col, col_widths) as u16
                        + tv.table_text_skip,
                )
                .saturating_sub(h_scroll);

            let highlight_line = search_query.and_then(|q| {
                let starts = super::search::match_starts_in_cell(search_matches, row_idx, col);
                if starts.is_empty() {
                    None
                } else {
                    Some((q, starts))
                }
            });
            // Matched text uses a uniform accent style so it reads against the
            // pane background. On a selected row/column the accent fg would
            // clash with the selection background, so we keep the selection
            // foreground and mark the hit with bold instead. The *current* match
            // cell is distinguished solely by its frame (`match_cell_border_style`),
            // so both highlight args below use the same text style.
            let match_text_style = if row_selected || is_active {
                base_style.add_modifier(Modifier::BOLD)
            } else {
                p.match_style()
            };
            let line = if let Some((q, starts)) = highlight_line {
                super::search::cell_highlight_line(
                    value,
                    text_skip,
                    tv.text_w,
                    q,
                    &starts,
                    current_match
                        .filter(|m| m.row == row_idx && m.col == col)
                        .map(|m| m.start),
                    base_style,
                    match_text_style,
                    match_text_style,
                )
            } else {
                Line::from(Span::styled(
                    crate::common::view::format::truncate_cell_display_from(
                        value,
                        text_skip,
                        tv.text_w,
                    ),
                    base_style,
                ))
            };
            frame.render_widget(
                Paragraph::new(line),
                Rect::new(col_x, y_base, tv.text_w, RESULTS_ROW_CONTENT_HEIGHT),
            );

            // Column border for this row.
            let col_right = crate::common::view::format::col_x_end(col, col_widths);
            let border_x = table_area
                .x
                .saturating_add(col_right as u16)
                .saturating_sub(h_scroll)
                .saturating_sub(1);
            if border_x >= table_area.x && border_x < table_area.x + table_area.width {
                for y in y_base..(y_base + RESULTS_ROW_HEIGHT).min(table_area.bottom()) {
                    frame
                        .buffer_mut()
                        .set_string(border_x, y, "│", grid_style);
                }
            }

            // Recolor the surrounding grid border into an accent frame for the
            // current-match cell so it stands out (drawn after the plain border).
            // The bottom edge is tinted later by the row separator.
            if is_match_cell {
                match_bottom_span = draw_match_cell_frame(
                    frame,
                    current_match_border_style(p),
                    table_area,
                    col_x,
                    y_base,
                    border_x,
                );
            }
        }

        // Row separator line (tinted as the current-match cell's bottom border).
        let row_sep_y = y_base + RESULTS_ROW_CONTENT_HEIGHT;
        if row_sep_y < table_area.bottom() {
            let accent = current_match_border_style(p);
            let sep_right = table_area.x.saturating_add(content_width);
            for x in table_area.x..sep_right.min(table_area.right()) {
                let (glyph, style) = match match_bottom_span {
                    Some((l, r)) if x >= l && x <= r => {
                        let c = if x == l { "└" } else if x == r { "┘" } else { "─" };
                        (c, accent)
                    }
                    _ => ("─", grid_style),
                };
                frame.buffer_mut().set_string(x, row_sep_y, glyph, style);
            }
        }
    }

    // Vertical scrollbar for the table rows.
    if let Some(bar) = layout.v_scrollbar {
        crate::common::view::pane_scrollbar::draw_vertical_pane_scrollbar(
            frame,
            bar,
            v_scroll,
            visible_data_rows,
            vs.max_v_scroll,
            p,
            false,
        );
    }

    // Horizontal scrollbar for columns that overflow the viewport.
    if let Some(bar) = layout.h_scrollbar {
        crate::common::view::pane_scrollbar::draw_horizontal_pane_scrollbar(
            frame,
            bar,
            h_scroll as usize,
            table_area.width as usize,
            vs.max_h_scroll,
            p,
            false,
        );
    }

    // Sync computed scroll values and viewport info back to state so the next
    // frame starts from the correct position (fixes stale h_scroll issue).
    // Uses Cell for interior mutability — allows writing through &ListState.
    state.h_scroll.set(vs.h_scroll);
    state.v_scroll.set(v_scroll);
    state.viewport_width.set(table_area.width);
    state.viewport_rows.set(visible_data_rows);
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

    let visible_data_rows = usize::from(
        content_area
            .height
            .saturating_sub(RESULTS_HEADER_HEIGHT)
            / RESULTS_ROW_HEIGHT,
    );
    let vr = visible_data_rows.max(1);
    let max_v_scroll = row_count.saturating_sub(vr);
    let max_h_scroll = crate::common::view::format::results_max_h_scroll(table_width as u16, content_area.width)
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

/// Hit-test a click at `(x, y)` inside the list's **inner** area (already
/// inside the outer Results Block). Returns `Some((row, col))` when the
/// click lands on a data cell (not header, action bar, pagination, footer,
/// or scrollbar), or `None` otherwise.
///
/// Uses [`compute_table_area`] so hit-test geometry always matches the
/// renderer's split logic exactly.
pub fn cell_hit_at(
    inner: Rect,
    state: &ListState,
    x: u16,
    y: u16,
    detail_open: bool,
) -> Option<(usize, usize)> {
    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    let result = state.result.as_ref()?;
    if result.columns.is_empty() || result.rows.is_empty() {
        return None;
    }

    let col_widths = &state.col_widths;

    // Use the single source of truth for geometry.
    let (table_area, _action_bar_area, _pagination_area, _footer_area) = compute_table_area(
        inner,
        state.row_count(),
        state.search.text_input_active(),
        detail_open,
        &state.executed_sql_display(),
    );

    let row_count = result.rows.len();
    let table_width = crate::common::view::format::results_table_width(col_widths);

    // ---- SHARED VIEWPORT CALCULATION ----
    // Use the exact same compute_viewport_scroll that render_table uses —
    // no more duplicated anchor logic that silently drifts from render.
    let vs = compute_viewport_scroll(table_area, state, row_count, col_widths, table_width as usize)?;

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

fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x.saturating_add(r.width) && y >= r.y && y < r.y.saturating_add(r.height)
}
