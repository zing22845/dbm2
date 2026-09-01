//! Editor feature rendering.
//!
//! Renders the SQL editor buffer (via the shared `render_editor`), the SQL
//! completion popup over it, and the context picker panel.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::common::components::search::{pane_search_bottom_title_line, pane_search_label_line};
use crate::common::editor;
use crate::common::view::hints::{draw_pane_footer, footer_height, sql_pane_footer_text};
use crate::common::view::pane_scrollbar::{
    draw_vertical_pane_scrollbar, pane_scroll_layout,
};
use crate::common::view::theme::Theme;

use super::state::EditorState;
use super::context_picker::view as cp_view;
use super::sql_completion::view as sc_view;

/// Render the editor feature. Returns the hardware cursor position if the
/// editor is visible (the caller places the terminal cursor). `focused` drives
/// the border/title highlight (the current SQL sub-pane is emphasized, others
/// muted), matching the other panes.
/// The height of the context picker overlay panel, matching the original dbm
/// (a 7-row horizontal panel at the top of the SQL editor).
pub const CONTEXT_PICKER_HEIGHT: u16 = 7;

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
    let prefix_len = crate::common::utils::text_width::width(&format!(" [S] SQL [{}] ", editor_mode_label(mode))) as u16;
    let x = area.x.saturating_add(1).saturating_add(prefix_len);
    let db_seg = format!("· {db}");
    let ctx = format!("{db_seg} › {schema}");
    let db_rect = Rect::new(x, area.y, crate::common::utils::text_width::width(&db_seg) as u16, 1);
    let full_rect = Rect::new(x, area.y, crate::common::utils::text_width::width(&ctx) as u16, 1);
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
    let full_title = format!(" [S] SQL [INSERT] · {db} › {schema} · TblCmp:{}", if complete_table_names { "ON" } else { "OFF" });
    let chip_text = format!("· TblCmp:{}", if complete_table_names { "ON" } else { "OFF" });
    let total_w = crate::common::utils::text_width::width(&full_title) as u16;
    let chip_w = crate::common::utils::text_width::width(&chip_text) as u16;
    let x = area.x.saturating_add(1).saturating_add(total_w).saturating_sub(chip_w);
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

    let editor_body = if picker_area.is_some() { chunks[1] } else { chunks[0] };
    let footer_area = if picker_area.is_some() { chunks[2] } else { chunks[1] };

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
    let provisional_row_count = editor::editor_display_row_count(editor_state, provisional_wrap as u16);
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

/// The all-caps mode label shown in the editor header, matching the original
/// dbm (`[INSERT]` / `[VISUAL]` / `[SEARCH]` / `[NORMAL]`).
fn editor_mode_label(mode: edtui::EditorMode) -> &'static str {
    match mode {
        edtui::EditorMode::Insert => "INSERT",
        edtui::EditorMode::Visual => "VISUAL",
        edtui::EditorMode::Search => "SEARCH",
        edtui::EditorMode::Normal => "NORMAL",
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &EditorState,
    focused: bool,
    complete_table_names: bool,
    database: Option<&str>,
    schema: Option<&str>,
) -> Option<crate::common::editor::EditorHardwareCursor> {
    let p = theme.palette();

    // The editor gets a titled border like the results / history panes, with
    // the current edit mode (all-caps), the connection context (database ›
    // schema, clickable to switch), and — in INSERT mode — the table-name
    // completion (TblCmp) status.
    let mode = editor_mode_label(state.editor.mode);
    let mut base = format!(" [S] SQL [{mode}]");
    let db = database.filter(|d| !d.is_empty()).unwrap_or("…");
    let schema = schema.filter(|s| !s.is_empty()).unwrap_or("…");
    base.push_str(&format!(" · {db} › {schema}"));
    if state.editor.mode == edtui::EditorMode::Insert {
        let flag = if complete_table_names { "ON" } else { "OFF" };
        base.push_str(&format!(" · TblCmp:{flag}"));
    }
    let title = pane_search_label_line(
        &base,
        focused,
        true,
        Style::default().fg(if focused { p.border_active } else { p.muted }),
        Some(Style::default().fg(if focused { p.accent } else { p.muted })),
    );

    // The pane search `/query [n/m]` renders on the bottom border
    // (`title_bottom`), matching the original dbm's search placement.
    let search_title = pane_search_bottom_title_line(
        &state.sql_search.search,
        focused,
        0,
        0,
        Some(area.width.saturating_sub(2)),
        Style::default().fg(p.muted),
    );

    let mut block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    if let Some(line) = search_title {
        block = block.title_bottom(line);
    }
    frame.render_widget(block, area);

    // Compute body/footer/picker geometry from the single source of truth.
    let search_active = state.sql_search.text_input_active();
    let (editor_body, footer_area, picker_area) = compute_editor_body_area(
        area,
        state.editor.mode,
        state.context_picker.open,
        complete_table_names,
        search_active,
        state.sql_search.has_filter(),
    );

    if let Some(picker_area) = picker_area {
        cp_view::render(frame, theme, picker_area, &state.context_picker);
    }

    // —— Scrollbar layout for the editor body ——
    // We use wrap(true), so edtui wraps lines to `content_main.width` which is
    // `content_area.width - gutter_width`. We must compute row_count at THAT
    // width, otherwise the scrollbar's max_scroll will be smaller than edtui's
    // actual total visual rows — making the last lines unreachable by drag.
    // Two-pass: first guess scrollbar width (1 col), compute row_count, then
    // refine with the actual scrollbar decision.
    let gutter_w = editor::editor_line_number_gutter_width(&state.editor);
    let viewport_rows = editor_body.height.max(1) as usize;

    // Pass 1: provisional row_count assuming v-scrollbar present.
    let provisional_wrap = editor_body
        .width
        .saturating_sub(1)
        .saturating_sub(gutter_w)
        .max(1);
    let provisional_row_count = editor::editor_display_row_count(&state.editor, provisional_wrap);
    let needs_v = provisional_row_count > viewport_rows;

    let scrollbar_w: u16 = if needs_v { 1 } else { 0 };
    let actual_wrap = editor_body
        .width
        .saturating_sub(scrollbar_w)
        .saturating_sub(gutter_w)
        .max(1);
    let row_count = editor::editor_display_row_count(&state.editor, actual_wrap);

    let scroll_layout = pane_scroll_layout(
        editor_body,
        // content_width = the actual wrap width edtui uses. This must NOT be
        // editor_body.width, otherwise pane_scroll_layout's needs_h check
        // (`content_width > main.width`) would falsely trigger an h-scrollbar
        // whenever a v-scrollbar is present — editor_body.width is always 1 col
        // wider than main.width when v_bar is reserved.
        actual_wrap,
        row_count,
        viewport_rows,
    );

    // Render the editor into the content area (minus scrollbar space).
    let mut editor = state.editor.clone();
    let content_area = scroll_layout.content_area;
    let cursor = editor::render_editor(&mut editor, content_area, frame.buffer_mut());

    // Draw the vertical scrollbar thumb after render so its position reflects
    // the final viewport_offset (edtui may nudge it to keep the cursor visible).
    if let Some(v_bar) = scroll_layout.v_scrollbar {
        let (_, scroll_y) = editor.viewport_offset();
        let max_scroll = row_count.saturating_sub(content_area.height.max(1) as usize);
        draw_vertical_pane_scrollbar(
            frame,
            v_bar,
            scroll_y,
            content_area.height.max(1) as usize,
            max_scroll,
            p,
            false, // TODO: pass dragging state when editor scrollbar drag is wired
        );
    }
    // Anchor the completion popup to the editor cursor (its screen position) so
    // it follows the caret, matching the original dbm. `cursor.position` is the
    // absolute terminal position of the caret after rendering.
    sc_view::render(
        frame,
        theme,
        editor_body,
        &state.sql_completion,
        cursor.as_ref().map(|c| c.position),
    );
    // Editor footer hints from the shared builder (wrapped to the pane width).
    let footer_mode = match state.editor.mode {
        edtui::EditorMode::Insert => "insert",
        edtui::EditorMode::Visual => "visual",
        _ => "normal",
    };
    let hint = sql_pane_footer_text(
        search_active,
        search_active,
        footer_mode,
        state.sql_search.has_filter(),
        complete_table_names,
    );
    draw_pane_footer(frame, theme, footer_area, &hint);

    // Hand the hardware cursor up so the shell can place the terminal caret at
    // the editor cursor (it also drives the completion popup anchor above).
    cursor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_mode_label_is_uppercase() {
        // The editor header shows the mode all-caps, matching the original dbm.
        assert_eq!(editor_mode_label(edtui::EditorMode::Insert), "INSERT");
        assert_eq!(editor_mode_label(edtui::EditorMode::Visual), "VISUAL");
        assert_eq!(editor_mode_label(edtui::EditorMode::Search), "SEARCH");
        assert_eq!(editor_mode_label(edtui::EditorMode::Normal), "NORMAL");
    }

    #[test]
    fn context_picker_area_is_top_full_width_panel() {
        // Closed -> None.
        assert_eq!(context_picker_area(Rect::new(0, 0, 100, 30), false), None);
        // Open -> a 7-row, full-inner-width panel just below the block header.
        let area = context_picker_area(Rect::new(10, 5, 100, 30), true).unwrap();
        assert_eq!(area.x, 11);
        assert_eq!(area.y, 6);
        assert_eq!(area.width, 98);
        assert_eq!(area.height, 7);
    }

    #[test]
    fn context_trigger_rects_splits_db_and_schema_segments() {
        let area = Rect::new(0, 0, 120, 20);
        // Database present -> both rects at the header row, db rect leading.
        let (db_rect, full_rect) = context_trigger_rects(
            area,
            edtui::EditorMode::Normal,
            Some("mydb"),
            Some("public"),
        );
        assert_eq!(db_rect.y, area.y);
        assert_eq!(full_rect.y, area.y);
        // Width is display-cell width, not byte length (`·` is 2 bytes / 1 cell).
        assert_eq!(db_rect.width, "· mydb".chars().count() as u16);
        assert_eq!(db_rect.x, full_rect.x);
        assert!(full_rect.width > db_rect.width);
        // No database yet -> the trigger is still present (the picker is how you
        // choose one), matching the original dbm.
        let (db_rect, full_rect) =
            context_trigger_rects(area, edtui::EditorMode::Normal, None, None);
        assert!(db_rect.width > 0);
        assert!(full_rect.width > db_rect.width);
    }

    #[test]
    fn editor_render_does_not_hang_at_narrow_width() {
        // Regression: rendering the editor with real SQL at the very narrow
        // width the widened History zone leaves it used to hang (100% CPU).
        // A wide history pane (e.g. 84) squeezes the editor; edtui's wrapped
        // render must still terminate.
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let editor = EditorState::with_sql(
            "SELECT * FROM \"测试表\" WHERE id = 1 AND name ILIKE '%foo%' ORDER BY created_at DESC",
        );
        let theme = crate::common::view::theme::dracula();
        let mut terminal = Terminal::new(TestBackend::new(60, 40)).unwrap();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let _ = render(
                        frame,
                        &theme,
                        Rect::new(0, 0, 60, 40),
                        &editor,
                        false,
                        true,
                        None,
                        None,
                    );
                })
                .unwrap();
        }));
        assert!(r.is_ok(), "editor render hung or panicked at a narrow width");
    }
}
