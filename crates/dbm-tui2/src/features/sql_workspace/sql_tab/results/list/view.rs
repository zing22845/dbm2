//! Results list sub-module rendering: the action bar, the result table,
//! the pagination toolbar, and the list footer.
//!
//! The entire list (action bar + table + pagination + footer) is wrapped
//! in a single outer `Block` that matches the original dbm layout.

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

    // Outer Block: wraps action bar + table + pagination + footer, matching
    // the original dbm results layout.
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
    let model = toolbar_model(state);
    let max_bar_scroll = action_bar_width(&model).saturating_sub(content.width);
    let bar_scroll = state.h_scroll.min(max_bar_scroll as usize) as u16;

    let list_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(RESULTS_ACTION_BAR_HEIGHT),
            Constraint::Min(0),
        ])
        .split(content);

    draw_action_bar(frame, list_chunks[0], &model, bar_scroll, p);
    render_table(frame, theme, list_chunks[1], state, result, focused);

    // Pagination toolbar (full width, below the content area, inside the Block).
    if let Some(pag_area) = pagination_area {
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
        frame.render_widget(Paragraph::new(toolbar), pag_area);
    }

    // Results list footer (full width, inside the Block).
    draw_footer(frame, theme, footer_area, &hint);
}

/// Derive the toolbar enable/disable model from the current result/edit state.
fn toolbar_model(state: &ListState) -> ResultsToolbarModel {
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

/// Render the result table body directly into `area` (no own Block/borders).
/// The outer Block with title is created by the caller (`render`).
fn render_table(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ListState,
    result: &QueryResultData,
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

    let row_count = state.row_count();
    let layout = crate::common::view::pane_scrollbar::pane_scroll_layout(
        area,
        area.width,
        row_count,
        area.height as usize,
    );
    let table_area = layout.content_area;
    let visible_rows = (table_area.height as usize).min(row_count.max(1));
    let start_row = state.row.saturating_sub(visible_rows.saturating_sub(1) / 2);

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
