//! Editor pane geometry: the context picker, the completion toggle, the
//! body area and its scrollbar.

use crate::common::editor;
use crate::common::layout::pane_scrollbar::pane_scroll_layout;
use crate::common::layout::text::footer_height;
use crate::common::view::hints::sql_pane_footer_text;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::{Block, Borders};

/// The rect of the context picker overlay inside the editor block. It is a
/// full-width horizontal panel at the top of the editor's inner area, just
/// below the block's header border. Returns `None` when the picker is closed
/// or the area is too small to host it.
pub fn context_picker_area(area: Rect, open: bool) -> Option<Rect> {
    if !open {
        return None;
    }
    let inner = Block::default().borders(Borders::ALL).inner(area);
    let picker_h = CONTEXT_PICKER_HEIGHT.min(inner.height);
    Some(Rect::new(inner.x, inner.y, inner.width, picker_h))
}

/// The rects of the two horizontal sub-segments in the editor header trigger:
/// the leading `· {db}` segment (clicking it focuses the Database column) and
/// the whole `· {db} › {schema}` trigger (clicking the remainder focuses the
/// Schema column). The trigger is always present (even before a database is
/// chosen — the picker's whole purpose is to choose one), matching the original
/// dbm where the label renders unconditionally.
pub fn context_trigger_rects(
    area: Rect,
    mode: edtui::EditorMode,
    database: Option<&str>,
    schema: Option<&str>,
) -> (Rect, Rect) {
    let db = database.filter(|d| !d.is_empty()).unwrap_or("…");
    let schema = schema.filter(|s| !s.is_empty()).unwrap_or("…");
    // Widths must be display-cell widths, not byte lengths: `·`/`›` are UTF-8
    // multi-byte (2 bytes) yet render as one cell, so using `str::len()` would
    // over-cover the clickable region and mis-hit the schema segment as the
    // database column.
    let prefix_len = crate::common::utils::text_width::width(&format!(
        " [S] SQL [{}] ",
        editor_mode_label(mode)
    )) as u16;
    let x = area.x.saturating_add(1).saturating_add(prefix_len);
    let db_seg = format!("· {db}");
    let ctx = format!("{db_seg} › {schema}");
    let db_rect = Rect::new(
        x,
        area.y,
        crate::common::utils::text_width::width(&db_seg) as u16,
        1,
    );
    let full_rect = Rect::new(
        x,
        area.y,
        crate::common::utils::text_width::width(&ctx) as u16,
        1,
    );
    (db_rect, full_rect)
}

/// Hit-test rect for the `· TblCmp:ON` / `· TblCmp:OFF` chip appended to the
/// editor title in INSERT mode. Returns `None` when not in INSERT mode (the
/// chip is not rendered and Alt+Tab toggle is a no-op there anyway).
pub fn tblcmp_rect(
    area: Rect,
    mode: edtui::EditorMode,
    database: Option<&str>,
    schema: Option<&str>,
    complete_table_names: bool,
) -> Option<Rect> {
    if mode != edtui::EditorMode::Insert {
        return None;
    }
    let db = database.filter(|d| !d.is_empty()).unwrap_or("…");
    let schema = schema.filter(|s| !s.is_empty()).unwrap_or("…");
    let full_title = format!(
        " [S] SQL [INSERT] · {db} › {schema} · TblCmp:{}",
        if complete_table_names { "ON" } else { "OFF" }
    );
    let chip_text = format!(
        "· TblCmp:{}",
        if complete_table_names { "ON" } else { "OFF" }
    );
    let total_w = crate::common::utils::text_width::width(&full_title) as u16;
    let chip_w = crate::common::utils::text_width::width(&chip_text) as u16;
    let x = area
        .x
        .saturating_add(1)
        .saturating_add(total_w)
        .saturating_sub(chip_w);
    Some(Rect::new(x, area.y, chip_w, 1))
}

/// Single source of truth for how the editor block area is split into
/// picker overlay (if open), editor body, and footer hints. Both [`render`]
/// and the hit-test path in [`crate::features::sql_workspace::sql_tab::view`]
/// must call this function — independently re-deriving the layout causes
/// scrollbar geometry to drift when footer height or picker state changes.
///
/// Returns `(editor_body, footer_area, picker_area)`. `picker_area` is `None`
/// when the context picker is closed.
pub fn compute_editor_body_area(
    area: Rect,
    mode: edtui::EditorMode,
    context_picker_open: bool,
    complete_table_names: bool,
    sql_search_active: bool,
    sql_search_has_filter: bool,
) -> (Rect, Rect, Option<Rect>) {
    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(area);

    let footer_mode = match mode {
        edtui::EditorMode::Insert => "insert",
        edtui::EditorMode::Visual => "visual",
        _ => "normal",
    };
    let hint = sql_pane_footer_text(
        sql_search_active,
        sql_search_active,
        footer_mode,
        sql_search_has_filter,
        complete_table_names,
    );
    let footer_h = footer_height(&hint, inner.width).min(inner.height.saturating_sub(1));

    let mut constraints = Vec::new();
    let mut picker_area = None;
    if context_picker_open {
        let picker_h = CONTEXT_PICKER_HEIGHT.min(inner.height);
        constraints.push(Constraint::Length(picker_h));
        picker_area = Some(Rect::new(inner.x, inner.y, inner.width, picker_h));
    }
    constraints.push(Constraint::Min(0));
    constraints.push(Constraint::Length(footer_h));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    let editor_body = if picker_area.is_some() {
        chunks[1]
    } else {
        chunks[0]
    };
    let footer_area = if picker_area.is_some() {
        chunks[2]
    } else {
        chunks[1]
    };

    (editor_body, footer_area, picker_area)
}

/// Compute the editor body's scrollbar geometry for hit-testing and drag
/// dispatch. Returns `Some((v_scrollbar_rect, max_scroll))` when a vertical
/// scrollbar is actually visible (content rows exceed the viewport).
///
/// `editor_body` is the full body rect (inside the border block), the same
/// area that `render` passes to `pane_scroll_layout`.
pub fn editor_body_v_scrollbar_info(
    editor_body: Rect,
    editor_state: &edtui::EditorState,
) -> Option<(Rect, usize)> {
    // Match edtui's actual wrap width: editor_body minus any reserved v-scrollbar
    // minus the line-number gutter. We use the same provisional two-pass logic
    // as the render path so both agree on row_count and max_scroll.
    let gutter_w = editor::editor_line_number_gutter_width(editor_state);
    let viewport_rows = editor_body.height.max(1) as usize;

    // Provisional wrap width assuming a scrollbar will be needed.
    let provisional_wrap = editor_body
        .width
        .saturating_sub(1)
        .saturating_sub(gutter_w)
        .max(1) as usize;
    let provisional_row_count =
        editor::editor_display_row_count(editor_state, provisional_wrap as u16);
    let needs_v = provisional_row_count > viewport_rows;

    let scrollbar_w: u16 = if needs_v { 1 } else { 0 };
    let actual_wrap = editor_body
        .width
        .saturating_sub(scrollbar_w)
        .saturating_sub(gutter_w)
        .max(1);
    let row_count = editor::editor_display_row_count(editor_state, actual_wrap);
    let max_scroll = row_count.saturating_sub(viewport_rows);
    if max_scroll == 0 {
        return None;
    }
    let layout = pane_scroll_layout(
        editor_body,
        // content_width is the ACTUAL wrap width (the max line width that edtui
        // uses internally). Passing this prevents pane_scroll_layout from falsely
        // reserving an h-scrollbar when content_width would otherwise equal
        // editor_body.width (which is wider than the actual wrapped content).
        actual_wrap,
        row_count,
        viewport_rows,
    );
    layout.v_scrollbar.map(|bar| (bar, max_scroll))
}

/// Render the editor feature. Returns the hardware cursor position if the
/// editor is visible (the caller places the terminal cursor). `focused` drives
/// the border/title highlight (the current SQL sub-pane is emphasized, others
/// muted), matching the other panes.
/// The height of the context picker overlay panel, matching the original dbm
/// (a 7-row horizontal panel at the top of the SQL editor).
pub const CONTEXT_PICKER_HEIGHT: u16 = 7;

/// The all-caps mode label shown in the editor header, matching the original
/// dbm (`[INSERT]` / `[VISUAL]` / `[SEARCH]` / `[NORMAL]`).
pub(super) fn editor_mode_label(mode: edtui::EditorMode) -> &'static str {
    match mode {
        edtui::EditorMode::Insert => "INSERT",
        edtui::EditorMode::Visual => "VISUAL",
        edtui::EditorMode::Search => "SEARCH",
        edtui::EditorMode::Normal => "NORMAL",
    }
}
