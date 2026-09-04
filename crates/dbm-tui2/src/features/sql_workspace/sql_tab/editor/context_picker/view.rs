//! Context picker sub-module rendering.
//!
//! Renders the database + schema selection overlay as two side-by-side
//! bordered panels. Each panel's column label sits on the top border title,
//! while its `/` search and filter counter render on the bottom border
//! (via `pane_search_bottom_title_line`), matching every other pane. The
//! focused column has an active border. The picker renders nothing when closed.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::components::search::{PaneSearch, pane_search_bottom_title_line, pane_search_label_line};
use crate::common::view::theme::Theme;

use super::state::{CachedList, ContextPickerState, PickerColumn, filter_indices};

/// The two column rects (database, schema) of the picker overlay, from the
/// shared `context_picker_area`. Used by both render and the click handler.
pub fn column_rects(area: Rect) -> (Rect, Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    (columns[0], columns[1])
}

/// Hit-test the picker rows at `(x, y)`. Returns `(column, filtered-index)` for
/// a row that is currently visible, or `None` when the picker is closed / the
/// point is not on a row. Mirrors the geometry produced by `picker_list_lines`.
pub fn row_hit_at(area: Rect, state: &ContextPickerState, x: u16, y: u16) -> Option<(PickerColumn, usize)> {
    if !state.open {
        return None;
    }
    let (db_rect, schema_rect) = column_rects(area);
    let db_block_inner = Block::default().borders(Borders::ALL).inner(db_rect);
    let schema_block_inner = Block::default().borders(Borders::ALL).inner(schema_rect);
    if let Some(idx) = column_row_hit(&state.databases, &state.db_search, state.db_cursor, db_block_inner, x, y) {
        return Some((PickerColumn::Database, idx));
    }
    if let Some(idx) = column_row_hit(&state.schemas, &state.schema_search, state.schema_cursor, schema_block_inner, x, y) {
        return Some((PickerColumn::Schema, idx));
    }
    None
}

/// Find the filtered-index of a row in one picker column whose rect contains
/// `(x, y)`.
fn column_row_hit(
    list: &CachedList,
    search: &PaneSearch,
    cursor: usize,
    inner: Rect,
    x: u16,
    y: u16,
) -> Option<usize> {
    if x < inner.x
        || x >= inner.x.saturating_add(inner.width)
        || y < inner.y
        || y >= inner.y.saturating_add(inner.height)
    {
        return None;
    }
    let CachedList::Ready(items) = list else {
        return None;
    };
    let filtered = filter_indices(items, search);
    if filtered.is_empty() {
        return None;
    }
    let window = inner.height.max(1) as usize;
    let cursor = cursor.min(filtered.len().saturating_sub(1));
    let start = cursor.saturating_sub(window / 2);
    let end = (start + window).min(filtered.len());
    let start = end.saturating_sub(window);
    let offset = (y - inner.y) as usize;
    let row = start + offset;
    // The cursor is an index into the filtered list, matching `picker_list_lines`.
    if row < filtered.len() {
        Some(row)
    } else {
        None
    }
}

/// Render the context picker overlay (a no-op when closed).
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &ContextPickerState) {
    if !state.open {
        return;
    }
    let p = theme.palette();

    let (db_col, schema_col) = column_rects(area);
    let columns = [db_col, schema_col];

    let db_active = state.column == PickerColumn::Database;
    let schema_active = state.column == PickerColumn::Schema;

    let db_filtered = filtered_count(&state.databases, &state.db_search);
    let schema_filtered = filtered_count(&state.schemas, &state.schema_search);

    // Column label stays on the top title; the `/` search and its counter move
    // to the bottom border (title_bottom), matching every other pane. The label
    // is type/muted when its column is inactive and border-accent when focused.
    let db_label = pane_search_label_line(
        " databases ",
        db_active,
        true,
        Style::default().fg(p.muted),
        Some(Style::default().fg(p.border_active_popup)),
        Style::default().fg(p.accent),
    );
    let schema_label = pane_search_label_line(
        &format!(" schemas ({}) ", state.preview_database),
        schema_active,
        true,
        Style::default().fg(p.muted),
        Some(Style::default().fg(p.border_active_popup)),
        Style::default().fg(p.accent),
    );
    let db_search_title = pane_search_bottom_title_line(
        &state.db_search,
        state.db_cursor,
        db_filtered,
        Some(db_col.width.saturating_sub(2)),
        None,
        p.match_style(),
        p.current_match_style(),
    );
    let schema_search_title = pane_search_bottom_title_line(
        &state.schema_search,
        state.schema_cursor,
        schema_filtered,
        Some(schema_col.width.saturating_sub(2)),
        None,
        p.match_style(),
        p.current_match_style(),
    );

    let selected_style = Style::default()
        .fg(p.selection_text)
        .bg(p.selection_bg)
        .add_modifier(Modifier::BOLD);

    let mut db_block = Block::default()
        .title(db_label)
        .borders(Borders::ALL)
        .border_style(p.popup_border(db_active))
        .style(Style::default().bg(p.surface));
    if let Some(line) = db_search_title {
        db_block = db_block.title_bottom(line);
    }
    let db_inner = db_block.inner(columns[0]);
    let db_lines = picker_list_lines(&state.databases, &state.db_search, state.db_cursor, db_inner, db_active, selected_style, p);
    frame.render_widget(Paragraph::new(db_lines).block(db_block), columns[0]);

    let mut schema_block = Block::default()
        .title(schema_label)
        .borders(Borders::ALL)
        .border_style(p.popup_border(schema_active))
        .style(Style::default().bg(p.surface));
    if let Some(line) = schema_search_title {
        schema_block = schema_block.title_bottom(line);
    }
    let schema_inner = schema_block.inner(columns[1]);
    let schema_lines = picker_list_lines(&state.schemas, &state.schema_search, state.schema_cursor, schema_inner, schema_active, selected_style, p);
    frame.render_widget(Paragraph::new(schema_lines).block(schema_block), columns[1]);
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
