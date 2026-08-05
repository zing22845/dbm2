//! Context picker sub-module rendering.
//!
//! Renders the database + schema selection overlay as two side-by-side
//! bordered panels. Each panel's title carries the `/` search state and filter
//! counter (via the shared `pane_search_title_line`); the focused column has an
//! active border. The picker renders nothing when closed.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::components::search::{PaneSearch, pane_search_title_line};
use crate::common::view::theme::Theme;

use super::state::{CachedList, ContextPickerState, PickerColumn, filter_indices};

/// Render the context picker overlay (a no-op when closed).
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &ContextPickerState) {
    if !state.open {
        return;
    }
    let p = theme.palette();

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let db_active = state.column == PickerColumn::Database;
    let schema_active = state.column == PickerColumn::Schema;

    let db_filtered = filtered_count(&state.databases, &state.db_search);
    let schema_filtered = filtered_count(&state.schemas, &state.schema_search);

    let db_title = picker_column_title_line(
        " databases ",
        &state.db_search,
        state.db_cursor,
        db_filtered,
        db_active,
        Style::default().fg(p.muted),
        Style::default().fg(p.border_active),
    );
    let schema_title = picker_column_title_line(
        &format!(" schemas ({}) ", state.preview_database),
        &state.schema_search,
        state.schema_cursor,
        schema_filtered,
        schema_active,
        Style::default().fg(p.muted),
        Style::default().fg(p.border_active),
    );

    let selected_style = Style::default()
        .fg(p.selection)
        .add_modifier(Modifier::BOLD);

    let db_block = Block::default()
        .title(db_title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if db_active { p.border_active } else { p.border }))
        .style(Style::default().bg(p.surface));
    let db_inner = db_block.inner(columns[0]);
    let db_lines = picker_list_lines(&state.databases, &state.db_search, state.db_cursor, db_inner, db_active, selected_style, p);
    frame.render_widget(Paragraph::new(db_lines).block(db_block), columns[0]);

    let schema_block = Block::default()
        .title(schema_title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if schema_active { p.border_active } else { p.border }))
        .style(Style::default().bg(p.surface));
    let schema_inner = schema_block.inner(columns[1]);
    let schema_lines = picker_list_lines(&state.schemas, &state.schema_search, state.schema_cursor, schema_inner, schema_active, selected_style, p);
    frame.render_widget(Paragraph::new(schema_lines).block(schema_block), columns[1]);
}

/// The title line for one picker column: label + `/` search + filter counter.
fn picker_column_title_line(
    label: &str,
    search: &PaneSearch,
    cursor: usize,
    filtered_count: usize,
    column_focused: bool,
    theme_muted: Style,
    active_label_style: Style,
) -> Line<'static> {
    pane_search_title_line(
        label,
        search,
        column_focused,
        true,
        theme_muted,
        cursor,
        filtered_count,
        None,
        Some(active_label_style),
        None,
    )
}

/// Build the visible (windowed) list lines for one picker column.
fn picker_list_lines(
    list: &CachedList,
    search: &PaneSearch,
    cursor: usize,
    inner: Rect,
    column_active: bool,
    selected_style: Style,
    p: &crate::common::view::theme::Palette,
) -> Vec<Line<'static>> {
    match list {
        CachedList::Loading => vec![Line::from(Span::styled(
            "(loading…)",
            Style::default().fg(p.muted),
        ))],
        CachedList::Error(err) => vec![Line::from(Span::styled(
            format!("(error: {err})"),
            Style::default().fg(p.error),
        ))],
        CachedList::Ready(items) => {
            let filtered = filter_indices(items, search);
            if filtered.is_empty() {
                return vec![Line::from(Span::styled(
                    "(no matches)",
                    Style::default().fg(p.muted),
                ))];
            }
            let window = inner.height.max(1) as usize;
            let cursor = cursor.min(filtered.len().saturating_sub(1));
            let start = cursor.saturating_sub(window / 2);
            let end = (start + window).min(filtered.len());
            let start = end.saturating_sub(window);
            filtered[start..end]
                .iter()
                .enumerate()
                .map(|(offset, &idx)| {
                    let name = items[idx].clone();
                    let selected = start + offset == cursor;
                    let prefix = if selected { "▸ " } else { "  " };
                    let style = if selected && column_active {
                        selected_style
                    } else {
                        Style::default().fg(p.fg)
                    };
                    Line::from(Span::styled(format!("{prefix}{name}"), style))
                })
                .collect()
        }
    }
}

/// Number of filtered items in a `CachedList` (0 while loading/error).
fn filtered_count(list: &CachedList, search: &PaneSearch) -> usize {
    match list {
        CachedList::Ready(items) => filter_indices(items, search).len(),
        _ => 0,
    }
}
