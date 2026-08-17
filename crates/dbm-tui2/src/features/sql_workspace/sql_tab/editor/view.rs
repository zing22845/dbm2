//! Editor feature rendering.
//!
//! Renders the SQL editor buffer (via the shared `render_editor`), the SQL
//! completion popup over it, and the context picker panel.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::common::components::search::pane_search_title_line;
use crate::common::editor;
use crate::common::view::hints::{draw_footer, footer_height, sql_pane_footer_text};
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
    let prefix_len = format!(" [S] SQL [{}] ", editor_mode_label(mode)).len() as u16;
    let x = area.x.saturating_add(1).saturating_add(prefix_len);
    let db_seg = format!("· {db}");
    let ctx = format!("{db_seg} › {schema}");
    let db_rect = Rect::new(x, area.y, db_seg.len() as u16, 1);
    let full_rect = Rect::new(x, area.y, ctx.len() as u16, 1);
    (db_rect, full_rect)
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
    // The footer hint builder matches on the lowercase mode name.
    let footer_mode = match state.editor.mode {
        edtui::EditorMode::Insert => "insert",
        edtui::EditorMode::Visual => "visual",
        _ => "normal",
    };
    let mut base = format!(" [S] SQL [{mode}]");
    let db = database.filter(|d| !d.is_empty()).unwrap_or("…");
    let schema = schema.filter(|s| !s.is_empty()).unwrap_or("…");
    base.push_str(&format!(" · {db} › {schema}"));
    if state.editor.mode == edtui::EditorMode::Insert {
        let flag = if complete_table_names { "ON" } else { "OFF" };
        base.push_str(&format!(" · TblCmp:{flag}"));
    }
    let title = pane_search_title_line(
        &base,
        &state.sql_search.search,
        false,
        true,
        Style::default().fg(if focused { p.border_active } else { p.muted }),
        0,
        0,
        None,
        Some(Style::default().fg(if focused { p.accent } else { p.muted })),
        Some(Style::default().fg(if focused { p.accent } else { p.muted })),
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // The footer hint is sized to its wrapped height so a narrow terminal does
    // not clip it; the editor body gets the remaining space.
    let search_active = state.sql_search.text_input_active();
    let hint = sql_pane_footer_text(search_active, search_active, footer_mode, state.sql_search.has_filter());
    let footer_h = footer_height(&hint, inner.width).min(inner.height.saturating_sub(1));
    // When the context picker is open it occupies a full-width panel at the top
    // of the editor (below the block header), with the editor body + footer
    // filling the rest — matching the original dbm's `sql_tab_layout`.
    let mut constraints = Vec::new();
    let mut picker_area = None;
    if state.context_picker.open {
        let picker_h = CONTEXT_PICKER_HEIGHT.min(inner.height);
        constraints.push(Constraint::Length(picker_h));
        picker_area = Some(Rect::new(inner.x, inner.y, inner.width, picker_h));
    }
    constraints.push(Constraint::Min(0)); // editor body
    constraints.push(Constraint::Length(footer_h)); // editor footer hints
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    // The editor body is the first non-picker chunk.
    let editor_body = if picker_area.is_some() { chunks[1] } else { chunks[0] };
    let footer_area = if picker_area.is_some() { chunks[2] } else { chunks[1] };

    if let Some(picker_area) = picker_area {
        cp_view::render(frame, theme, picker_area, &state.context_picker);
    }

    let mut editor = state.editor.clone();
    let cursor = editor::render_editor(&mut editor, editor_body, frame.buffer_mut());
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
    draw_footer(frame, theme, footer_area, &hint);

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
        assert_eq!(db_rect.width, "· mydb".len() as u16);
        assert_eq!(db_rect.x, full_rect.x);
        assert!(full_rect.width > db_rect.width);
        // No database yet -> the trigger is still present (the picker is how you
        // choose one), matching the original dbm.
        let (db_rect, full_rect) =
            context_trigger_rects(area, edtui::EditorMode::Normal, None, None);
        assert!(db_rect.width > 0);
        assert!(full_rect.width > db_rect.width);
    }
}
