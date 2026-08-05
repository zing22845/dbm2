//! Results feature rendering: the result table, the pagination toolbar, and
//! the detail sub-pane.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::components::search::pane_search_title_line;
use crate::common::view::theme::Theme;

use super::state::ResultsState;
use super::detail::view as detail_view;
use super::pagination::{RESULTS_PAGINATION_BAR_HEIGHT, pagination_toolbar_line};

/// Render the results feature: table (or empty/error), toolbar, detail.
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &ResultsState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = theme.palette();

    let Some(result) = state.result.as_ref() else {
        render_empty(frame, theme, area);
        return;
    };

    let total_rows = result.total_rows;
    let row_count = state.row_count();
    let bar_h = RESULTS_PAGINATION_BAR_HEIGHT;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(bar_h), // pagination toolbar
            Constraint::Min(0),        // table
            Constraint::Length(8),     // detail
        ])
        .split(area);

    // Toolbar.
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

    // Table.
    render_table(frame, theme, chunks[1], state, result);

    // Detail.
    let body = state.selected_cell().unwrap_or_default();
    let col_name = state.selected_column_name().unwrap_or("").to_string();
    let title = format!(" [{}] row {}", if col_name.is_empty() { "?" } else { &col_name }, state.row + 1);
    detail_view::render(frame, theme, chunks[2], &state.detail, &body, title, true);
}

fn render_empty(frame: &mut Frame, theme: &Theme, area: Rect) {
    let p = theme.palette();
    let block = Block::default()
        .title(" Results ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border))
        .style(Style::default().bg(p.surface));
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
) {
    let p = theme.palette();
    let title = pane_search_title_line(
        " Results",
        &state.search,
        true,
        false,
        Style::default().fg(p.muted),
        state.row,
        state.row_count(),
        None,
        None,
        None,
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border_active))
        .style(Style::default().bg(p.surface));
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

    let visible_rows = (inner.height as usize).min(state.row_count().max(1));
    let start_row = state.row.saturating_sub(visible_rows.saturating_sub(1) / 2);
    let col_count = result.columns.len();
    let col_w = (inner.width as usize).saturating_sub(1).div_ceil(col_count.max(1)).max(4);
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
    frame.render_widget(Paragraph::new(Line::from(header)), inner);

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
                    let selected = row_idx == state.row && i == state.col;
                    let style = if selected {
                        Style::default()
                            .fg(p.selection)
                            .add_modifier(Modifier::BOLD)
                    } else if row_idx == state.row {
                        Style::default().fg(p.fg)
                    } else {
                        Style::default().fg(p.fg)
                    };
                    Span::styled(format!("{v} "), style)
                })
                .collect();
            Line::from(spans)
        })
        .collect();
    frame.render_widget(Paragraph::new(row_lines), inner);
}
