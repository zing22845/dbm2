//! Results feature rendering: the result table, the pagination toolbar, and
//! the detail sub-pane.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::components::search::pane_search_title_line;
use crate::common::view::action_bar::{
    RESULTS_ACTION_BAR_HEIGHT, ResultsToolbarModel, action_bar_width, draw_action_bar,
};
use crate::common::view::hints::{draw_footer, footer_height};
use crate::common::view::theme::Theme;

use super::state::ResultsState;
use super::detail::view as detail_view;
use super::pagination::{RESULTS_PAGINATION_BAR_HEIGHT, pagination_toolbar_line};

/// Render the results feature: table (or empty/error), toolbar, detail.
/// `focused` drives the border/title highlight so only the current SQL
/// sub-pane is emphasized.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ResultsState,
    focused: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = theme.palette();

    // A failed query is shown in the results pane as red error text, mirroring
    // the original dbm's `query_error` rendering (ui.rs render_results_plain).
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
    // The footer hint is sized to its wrapped height so a narrow terminal does
    // not clip it; the table absorbs the remaining space.
    let search_active = state.search.text_input_active();
    let hint = crate::common::view::hints::results_pane_footer_text(search_active, true, "");
    let footer_h = footer_height(&hint, area.width).min(area.height.saturating_sub(4));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(RESULTS_PAGINATION_BAR_HEIGHT), // pagination toolbar
            Constraint::Length(RESULTS_ACTION_BAR_HEIGHT),     // action bar
            Constraint::Min(0),                                // table
            Constraint::Length(8),                             // detail
            Constraint::Length(footer_h),                      // footer hints
        ])
        .split(area);

    // Pagination toolbar.
    let toolbar = pagination_toolbar_line(
        state.row_limit,
        state.page,
        total_rows,
        row_count,
        false,
        false,
        Style::default().fg(p.accent),
        Style::default().fg(p.muted),
    );
    frame.render_widget(Paragraph::new(toolbar), chunks[0]);

    // Action bar (Refresh / Edit / Inst / Dup / Del / Commit / Rollback).
    let model = toolbar_model(state);
    let max_bar_scroll = action_bar_width(&model).saturating_sub(chunks[1].width);
    let bar_scroll = state.h_scroll.min(max_bar_scroll as usize) as u16;
    draw_action_bar(frame, chunks[1], &model, bar_scroll, p);

    // Table.
    render_table(frame, theme, chunks[2], state, result, focused);

    // Detail.
    let body = state.selected_cell().unwrap_or_default();
    let col_name = state.selected_column_name().unwrap_or("").to_string();
    let title = format!(" [{}] row {}", if col_name.is_empty() { "?" } else { &col_name }, state.row + 1);
    detail_view::render(frame, theme, chunks[3], &state.detail, &body, title, true);

    // Results footer hints from the shared builder (the detail sub-pane is
    // always shown in this layout, so it is treated as open), wrapped to the
    // pane width.
    draw_footer(frame, theme, chunks[4], &hint);
}

/// Derive the toolbar enable/disable model from the current result/edit state.
fn toolbar_model(state: &ResultsState) -> ResultsToolbarModel {
    let has_result = state.result.is_some();
    let commit_n = state.commit_row_count();
    ResultsToolbarModel {
        refresh_enabled: has_result,
        edit_enabled: state.editable(),
        edit_active: state.edit.editing,
        commit_enabled: state.edit.editing && commit_n > 0,
        rollback_enabled: state.edit.editing && state.edit.is_dirty(),
        commit_n,
        edit_reason: state.edit_blocked_reason.clone(),
    }
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

fn render_table(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ResultsState,
    result: &super::state::QueryResultData,
    focused: bool,
) {
    let p = theme.palette();
    let title = pane_search_title_line(
        " [R] Results",
        &state.search,
        true,
        false,
        Style::default().fg(p.muted),
        state.row,
        state.row_count(),
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
            inner,
        );
        return;
    }

    // Reserve a scrollbar column when rows overflow the viewport.
    let row_count = state.row_count();
    let layout = crate::common::view::pane_scrollbar::pane_scroll_layout(
        inner,
        inner.width,
        row_count,
        inner.height as usize,
    );
    let table_area = layout.content_area;
    let visible_rows = (table_area.height as usize).min(row_count.max(1));
    let start_row = state.row.saturating_sub(visible_rows.saturating_sub(1) / 2);
    let col_count = result.columns.len();
    let col_w = (table_area.width as usize).saturating_sub(1).div_ceil(col_count.max(1)).max(4);
    let _ = col_w;

    // Header.
    let header: Vec<Span> = result
        .columns
        .iter()
        .enumerate()
        .map(|(i, meta)| {
            let name = meta.name.clone();
            let style = if i == state.col {
                Style::default().fg(p.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(p.muted)
            };
            Span::styled(name, style)
        })
        .collect();
    frame.render_widget(Paragraph::new(Line::from(header)), table_area);

    // Rows (windowed).
    let row_lines: Vec<Line> = result
        .rows
        .iter()
        .skip(start_row)
        .take(visible_rows)
        .enumerate()
        .map(|(off, row)| {
            let row_idx = start_row + off;
            let spans: Vec<Span> = row
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let in_selected_row = row_idx == state.row;
                    let selected_cell = in_selected_row && i == state.col;
                    // Every field in the selected row shares the unified
                    // selection background; the active cell gets a stronger
                    // `selection_cell_bg` so it stands out from its row-mates.
                    let style = if selected_cell {
                        Style::default()
                            .fg(p.fg)
                            .bg(p.selection_cell_bg)
                            .add_modifier(Modifier::BOLD)
                    } else if in_selected_row {
                        Style::default().fg(p.fg).bg(p.selection_bg)
                    } else {
                        Style::default().fg(p.fg)
                    };
                    Span::styled(format!("{v} "), style)
                })
                .collect();
            Line::from(spans)
        })
        .collect();
    frame.render_widget(Paragraph::new(row_lines), table_area);

    // Vertical scrollbar for the table rows.
    if let Some(bar) = layout.v_scrollbar {
        let max_scroll = row_count.saturating_sub(visible_rows);
        crate::common::view::pane_scrollbar::draw_vertical_pane_scrollbar(
            frame,
            bar,
            start_row,
            visible_rows,
            max_scroll,
            p,
            false,
        );
    }
}
