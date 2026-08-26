//! Results list sub-module rendering: the action bar, the result table,
//! the pagination toolbar, and the list footer.
//!
//! The entire list (action bar + table + pagination + footer) is wrapped
//! in a single outer `Block` that matches the original dbm layout.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::components::search::pane_search_title_line;
use crate::common::view::action_bar::{
    RESULTS_ACTION_BAR_HEIGHT, ResultsToolbarModel, action_bar_width, draw_action_bar,
};
use crate::common::view::format::{
    RESULTS_HEADER_HEIGHT, RESULTS_ROW_CONTENT_HEIGHT, RESULTS_ROW_HEIGHT,
    column_type_label, results_col_text_view,
};
use crate::common::view::hints::{draw_footer, footer_height};
use crate::common::view::theme::Theme;
use crate::common::view::pane_scrollbar::pane_scroll_layout;

use super::state::ListState;
use super::super::pagination::{RESULTS_PAGINATION_BAR_HEIGHT, pagination_toolbar_line};
use super::super::state::QueryResultData;

/// Render the list sub-feature: action bar + table + pagination + footer,
/// all wrapped inside a single outer Block with borders and title.
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
        render_error(frame, theme, area, focused, message);
        return;
    }

    let Some(result) = state.result.as_ref() else {
        render_empty(frame, theme, area, focused);
        return;
    };

    let total_rows = result.total_rows;
    let row_count = state.row_count();
    let state_row = state.row;
    let state_col = state.col;
    let state_h_scroll = state.h_scroll;
    let state_v_scroll = state.v_scroll;

    // Outer Block: wraps action bar + table + pagination + footer, matching
    // the original dbm results layout.
    let title = pane_search_title_line(
        " [R] Results",
        &state.search,
        true,
        false,
        Style::default().fg(p.muted),
        state_row,
        row_count,
        None,
        None,
        Some(Style::default().fg(if focused { p.accent } else { p.muted })),
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let search_active = state.search.text_input_active();
    let hint = crate::common::view::hints::results_pane_footer_text(
        search_active,
        detail_open,
        "",
    );
    let footer_h = footer_height(&hint, inner.width).min(inner.height.saturating_sub(4));

    let pagination_h = if row_count > 0 {
        RESULTS_PAGINATION_BAR_HEIGHT
    } else {
        0
    };

    let chunks = if pagination_h > 0 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(pagination_h),
                Constraint::Length(footer_h),
            ])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(footer_h),
            ])
            .split(inner)
    };

    let content = chunks[0];
    let pagination_area = if pagination_h > 0 { Some(chunks[1]) } else { None };
    let footer_area = if pagination_h > 0 { chunks[2] } else { chunks[1] };

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
            action_bar_width(&model).saturating_sub(content.width),
            model,
        )
    };
    let bar_scroll = state_h_scroll.min(bar_scroll_max as usize) as u16;

    let list_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(RESULTS_ACTION_BAR_HEIGHT),
            Constraint::Min(0),
        ])
        .split(content);

    draw_action_bar(frame, list_chunks[0], &model, bar_scroll, p);
    render_table(
        frame,
        theme,
        list_chunks[1],
        result,
        &state.col_widths,
        state_row,
        state_col,
        state_h_scroll,
        state_v_scroll,
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
    draw_footer(frame, theme, footer_area, &hint);
}

fn render_error(frame: &mut Frame, theme: &Theme, area: Rect, focused: bool, message: &str) {
    let p = theme.palette();
    let block = Block::default()
        .title(" [R] Results ")
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            message,
            Style::default().fg(p.error),
        ))),
        inner,
    );
}

fn render_empty(frame: &mut Frame, theme: &Theme, area: Rect, focused: bool) {
    let p = theme.palette();
    let block = Block::default()
        .title(" [R] Results ")
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Run a query to see results",
            Style::default().fg(p.muted),
        ))),
        inner,
    );
}

/// Render the result table body directly into `area` (no own Block/borders).
/// The outer Block with title is created by the caller (`render`).
///
/// Layout (matching original dbm):
///   Header: 2 lines (name + type label) + 1 separator = 3 rows total
///   Each data row: 1 content line + 1 separator = 2 rows total
///   Column borders: │ character between columns
fn render_table(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    result: &QueryResultData,
    col_widths: &[u16],
    state_row: usize,
    state_col: usize,
    state_h_scroll: usize,
    state_v_scroll: usize,
    _focused: bool,
) {
    let p = theme.palette();

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
    let max_h_scroll = crate::common::view::format::results_max_h_scroll(table_width, area.width);

    // Reserve scrollbar area: pass the table's content width so h_scrollbar is
    // shown only when columns overflow the viewport.
    let layout = pane_scroll_layout(area, table_width, row_count, area.height as usize);
    let table_area = layout.content_area;

    if table_area.height < RESULTS_HEADER_HEIGHT {
        return;
    }

    // Determine how many data rows are visible.
    let visible_data_rows = if row_count > 0 {
        usize::from(table_area.height.saturating_sub(RESULTS_HEADER_HEIGHT) / RESULTS_ROW_HEIGHT)
    } else {
        0
    };

    // Auto-adjust h_scroll to keep cursor anchored: only scroll when cursor
    // exits the visible viewport (based on pixel positions, not column
    // indices — handles wide columns that are the sole visible column).
    let viewport_width = table_area.width as usize;
    let mut h_scroll_val = state_h_scroll;
    if viewport_width > 0 && !col_widths.is_empty() {
        let view_right = h_scroll_val.saturating_add(viewport_width);
        let cur_col_left = crate::common::view::format::col_x_start(state_col, col_widths);
        let cur_col_right = crate::common::view::format::col_x_end(state_col, col_widths);
        if cur_col_right <= h_scroll_val {
            h_scroll_val = cur_col_left;
        } else if cur_col_left >= view_right {
            h_scroll_val = cur_col_right.saturating_sub(viewport_width);
        }
        let max_h = crate::common::view::format::results_max_h_scroll(table_width, table_area.width) as usize;
        h_scroll_val = h_scroll_val.min(max_h);
    }
    let h_scroll = h_scroll_val as u16;

    // Auto-adjust v_scroll to keep cursor anchored: only scroll when cursor
    // exits the visible viewport (not always anchored to first row).
    let vr = visible_data_rows.max(1);
    let max_scroll = row_count.saturating_sub(vr);
    let mut v_scroll = state_v_scroll.min(max_scroll);
    if state_row >= v_scroll + vr {
        v_scroll = state_row.saturating_sub(vr.saturating_sub(1));
    } else if state_row < v_scroll {
        v_scroll = state_row;
    }
    v_scroll = v_scroll.min(max_scroll);

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
        let name_style = if col == state_col {
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD)
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

        // Type label (green).
        let type_label = column_type_label(meta);
        let type_text = crate::common::view::format::truncate_cell_display_from(
            &type_label,
            tv.table_text_skip,
            tv.text_w,
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                type_text,
                Style::default().fg(Color::Green),
            ))),
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
        for x in table_area.x..table_area.right() {
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

        // Row content area (1 line).
        let row_content_area = Rect::new(
            table_area.x,
            y_base,
            table_area.width,
            RESULTS_ROW_CONTENT_HEIGHT,
        );

        // Row background (highlight if selected).
        let row_selected = row_idx == state_row;
        if row_selected {
            frame.render_widget(
                ratatui::widgets::Block::default().style(Style::default().bg(p.selection_bg)),
                row_content_area,
            );
        }

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

            let is_active = state_col == col;
            let base_style = if row_selected && is_active {
                Style::default()
                    .fg(p.fg)
                    .bg(p.selection_cell_bg)
                    .add_modifier(Modifier::BOLD)
            } else if row_selected {
                Style::default().fg(p.fg)
            } else if is_active {
                Style::default()
                    .fg(p.accent)
                    .bg(p.selection_cell_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(p.fg)
            };

            let text = crate::common::view::format::truncate_cell_display_from(
                value,
                tv.table_text_skip,
                tv.text_w,
            );
            let col_x = table_area
                .x
                .saturating_add(
                    crate::common::view::format::col_x_start(col, col_widths) as u16
                        + tv.table_text_skip,
                )
                .saturating_sub(h_scroll);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(text, base_style))),
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
        }

        // Row separator line.
        let row_sep_y = y_base + RESULTS_ROW_CONTENT_HEIGHT;
        if row_sep_y < table_area.bottom() {
            for x in table_area.x..table_area.right() {
                frame.buffer_mut().set_string(x, row_sep_y, "─", grid_style);
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
            max_scroll,
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
            max_h_scroll as usize,
            p,
            false,
        );
    }
}
