//! Editor feature rendering.
//!
//! Renders the SQL editor buffer (via the shared `render_editor`), the SQL
//! completion popup over it, and the context picker panel.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders};

use crate::common::components::search::{pane_search_bottom_title_line, pane_search_label_line};
use crate::common::editor;
use crate::common::layout::pane_scrollbar::{ActiveScrollbar, pane_scroll_layout};
use crate::common::view::hints::{draw_pane_footer, sql_pane_footer_text};
use crate::common::view::pane_scrollbar::draw_vertical_pane_scrollbar;
use crate::common::view::theme::Theme;

use super::context_picker::view as cp_view;
use super::layout::{compute_editor_body_area, editor_mode_label};
use super::sql_completion::view as sc_view;
use super::state::EditorState;

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
    active_scrollbar: Option<ActiveScrollbar>,
) -> (
    Option<crate::common::editor::EditorHardwareCursor>,
    Option<crate::common::editor::EditorMouseHitArea>,
) {
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
        Style::default().fg(if focused {
            p.border_active_child
        } else {
            p.muted
        }),
        Some(Style::default().fg(if focused { p.accent } else { p.muted })),
        p.search_active_style(),
    );

    // The pane search `/query [n/m]` renders on the bottom border
    // (`title_bottom`), matching the original dbm's search placement.
    let search_title = pane_search_bottom_title_line(
        &state.sql_search.search,
        0,
        0,
        Some(area.width.saturating_sub(2)),
        None,
        p.match_style(),
        p.current_match_style(),
    );

    let mut block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(p.child_border(focused));
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
    // Re-apply theme-derived match highlight styles at render time (only the
    // view knows the active palette), overriding the neutral placeholders set
    // in the update path so the highlights follow the theme.
    if !state.sql_search.matches.is_empty() {
        editor.set_highlights(state.sql_search.palette_highlights(p));
    }
    let content_area = scroll_layout.content_area;
    let cursor = editor::render_editor(&mut editor, content_area, frame.buffer_mut());

    // Capture the rendered mouse hit region: edtui draws the line-number gutter
    // inside `content_area`, so its `screen_area` is `content_area` minus the
    // gutter. The viewport is the value the clone actually rendered with (its
    // auto-scroll may have nudged the persisted offset). The run loop stores
    // this so the pointer layer can feed mouse events back through edtui's own
    // coordinate conversion.
    let (_, rendered_viewport_y) = editor.viewport_offset();
    let mouse_hit_area = crate::common::editor::EditorMouseHitArea {
        text_area: Rect {
            x: content_area.x.saturating_add(gutter_w),
            y: content_area.y,
            width: content_area.width.saturating_sub(gutter_w),
            height: content_area.height,
        },
        viewport_y: rendered_viewport_y,
    };

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
            matches!(active_scrollbar, Some(ActiveScrollbar::SqlV)),
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
    (cursor, Some(mouse_hit_area))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sql_workspace::sql_tab::editor::layout::{
        context_picker_area, context_trigger_rects,
    };

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
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let editor = EditorState::with_sql(
            "SELECT * FROM \"测试表\" WHERE id = 1 AND name ILIKE '%foo%' ORDER BY created_at DESC",
        );
        let theme = crate::common::view::theme::default();
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
                        None,
                    );
                })
                .unwrap();
        }));
        assert!(
            r.is_ok(),
            "editor render hung or panicked at a narrow width"
        );
    }
}
