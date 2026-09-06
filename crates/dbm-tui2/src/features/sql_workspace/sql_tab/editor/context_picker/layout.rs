//! Context picker geometry: row hit-testing and the column rects.

use super::state::{CachedList, ContextPickerState, PickerColumn, filter_indices};
use crate::common::components::search::PaneSearch;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::{Block, Borders};

/// Hit-test the picker rows at `(x, y)`. Returns `(column, filtered-index)` for
/// a row that is currently visible, or `None` when the picker is closed / the
/// point is not on a row. Mirrors the geometry produced by `picker_list_lines`.
pub fn row_hit_at(
    area: Rect,
    state: &ContextPickerState,
    x: u16,
    y: u16,
) -> Option<(PickerColumn, usize)> {
    if !state.open {
        return None;
    }
    let (db_rect, schema_rect) = column_rects(area);
    let db_block_inner = Block::default().borders(Borders::ALL).inner(db_rect);
    let schema_block_inner = Block::default().borders(Borders::ALL).inner(schema_rect);
    if let Some(idx) = column_row_hit(
        &state.databases,
        &state.db_search,
        state.db_cursor,
        db_block_inner,
        x,
        y,
    ) {
        return Some((PickerColumn::Database, idx));
    }
    if let Some(idx) = column_row_hit(
        &state.schemas,
        &state.schema_search,
        state.schema_cursor,
        schema_block_inner,
        x,
        y,
    ) {
        return Some((PickerColumn::Schema, idx));
    }
    None
}

/// The two column rects (database, schema) of the picker overlay, from the
/// shared `context_picker_area`. Used by both render and the click handler.
pub fn column_rects(area: Rect) -> (Rect, Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    (columns[0], columns[1])
}

/// Find the filtered-index of a row in one picker column whose rect contains
/// `(x, y)`.
pub(super) fn column_row_hit(
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
