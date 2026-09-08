//! Results list sub-module rendering: the action bar, the result table,
//! the pagination toolbar, and the list footer.
//!
//! The outer Block with border + title is drawn by the parent
//! `super::render()` — this module renders borderless content into the
//! already-inner area.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::common::layout::pane_scrollbar::ActiveScrollbar;
use crate::common::layout::text::footer_height;
use crate::common::model::RowChangeKind;
use crate::common::view::action_bar::draw_action_bar;
use crate::common::view::format::{
    RESULTS_HEADER_HEIGHT, RESULTS_ROW_CONTENT_HEIGHT, RESULTS_ROW_HEIGHT, column_type_label,
    results_col_text_view,
};
use crate::common::view::theme::Theme;

use super::super::pagination::RESULTS_PAGINATION_BAR_HEIGHT;
use super::layout::{
    compute_viewport_scroll, results_action_rows, results_geometry, results_list_regions,
    results_toolbar_model,
};
use super::state::ListState;

/// Render the list sub-feature: the action bar and the result table, borderless
/// — the outer Block with border + title, the full-width pagination toolbar,
/// and the full-width list footer are all drawn by the parent `results::render()`.
///
/// `list_area` is the content band (above the pagination toolbar / footer)
/// narrowed to the list side of the optional list|detail horizontal split.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    list_area: Rect,
    state: &ListState,
    focused: bool,
    col_resize: Option<usize>,
    active_scrollbar: Option<ActiveScrollbar>,
) {
    if list_area.width == 0 || list_area.height == 0 {
        return;
    }
    let p = theme.palette();

    // A failed query is shown as red error text.
    if let Some(message) = state.query_error.as_deref() {
        render_error(frame, theme, list_area, message);
        return;
    }

    let Some(_) = state.result.as_ref() else {
        render_empty(frame, theme, list_area);
        return;
    };

    // The list region splits into the action bar (top; buttons wrap onto extra
    // rows when the pane is narrow) and the table body.
    let model = results_toolbar_model(state);
    let action_rows = results_action_rows(list_area, state);
    let (action_bar_area, table_body) = results_list_regions(list_area, action_rows);

    draw_action_bar(frame, action_bar_area, &model, 0, p);
    render_table(
        frame,
        theme,
        table_body,
        state,
        focused,
        col_resize,
        active_scrollbar,
    );
}

/// Split the full-width Results Block inner area vertically into the content
/// band (which holds the list and, when open, the detail preview side by side)
/// above a full-width pagination toolbar and a full-width list footer. The
/// toolbar and footer span the whole inner width whether or not the detail is
/// open, so their width never changes.
///
/// Returns `(content, pagination, footer)`. `pagination` is `None` when there
/// are no rows to paginate.
pub fn results_vertical_layout(
    inner: Rect,
    row_count: usize,
    search_active: bool,
    sql_status: &str,
    extra_footer_rows: u16,
    next_modified: bool,
) -> (Rect, Option<Rect>, Rect) {
    let hint = crate::common::view::hints::results_pane_footer_text(
        search_active,
        sql_status,
        next_modified,
    );
    let footer_h = footer_height(&hint, inner.width)
        .saturating_add(extra_footer_rows)
        .min(inner.height.saturating_sub(4));

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
            .constraints([Constraint::Min(1), Constraint::Length(footer_h)])
            .split(inner)
    };

    let content = chunks[0];
    let pagination = if pagination_h > 0 {
        Some(chunks[1])
    } else {
        None
    };
    let footer = if pagination_h > 0 {
        chunks[2]
    } else {
        chunks[1]
    };
    (content, pagination, footer)
}

fn render_error(frame: &mut Frame, theme: &Theme, area: Rect, message: &str) {
    let p = theme.palette();
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            message,
            Style::default().fg(p.error),
        ))),
        area,
    );
}

fn render_empty(frame: &mut Frame, theme: &Theme, area: Rect) {
    let p = theme.palette();
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Run a query to see results",
            Style::default().fg(p.muted),
        ))),
        area,
    );
}

/// Compute the per-cell text skip (display cells) that keeps the current
/// match's `query` visible inside the match cell. Derived at render time — it
/// only has meaning for the cell holding the current match, so callers must
/// apply it only to that cell and never store it as global state.
///
/// `table_skip` is the column's existing horizontal skip (`tv.table_text_skip`);
/// the returned value cancels it so the match lands at `start - 1` in the cell
/// view, mirroring the original dbm.
fn match_cell_text_skip(
    value: &str,
    m: super::search::ResultsSearchMatch,
    query: &str,
    text_w: u16,
    table_skip: u16,
) -> u16 {
    let visible = text_w.saturating_sub(1);
    if visible == 0 || query.is_empty() {
        return 0;
    }
    let match_start_w = crate::common::view::format::display_width_char_prefix(value, m.start);
    let query_w = crate::common::view::format::cell_display_width(query);
    let max_skip = crate::common::view::format::max_cell_text_skip(value, visible);
    let ts = table_skip as usize;

    let mut cell_skip = match_start_w.saturating_sub(ts).saturating_sub(1);
    if match_start_w + query_w > cell_skip + ts + visible as usize {
        cell_skip = match_start_w
            .saturating_add(query_w)
            .saturating_sub(ts)
            .saturating_sub(visible as usize);
    }
    (cell_skip as u16).min(max_skip)
}

/// The frame around the current-match cell is drawn with the palette's match
/// border emphasis (accent) so it stands out from the muted grid.
fn current_match_border_style(p: &crate::common::view::theme::Palette) -> Style {
    p.match_cell_border_style()
}

/// Recolour the grid border around the current-match cell into an accent
/// frame: the top `─` (with corners) and the left/right `│`. The bottom edge
/// coincides with the row separator, which is tinted separately via the
/// returned `(left, right)` span. Skips cells that touch the pane edge where
/// there is no interior border to recolor.
fn draw_match_cell_frame(
    frame: &mut Frame,
    style: Style,
    table_area: Rect,
    content_x: u16,
    content_y: u16,
    border_x: u16,
) -> Option<(u16, u16)> {
    let left = content_x.saturating_sub(1);
    let right = border_x;
    let top = content_y.saturating_sub(1);
    let bottom = content_y.saturating_add(RESULTS_ROW_CONTENT_HEIGHT);
    if right <= left
        || left < table_area.x
        || right >= table_area.right()
        || bottom > table_area.bottom()
    {
        return None;
    }
    // Top edge + corners; bottom edge is the row separator (tinted by caller).
    frame.buffer_mut().set_string(left, top, "┌", style);
    for x in (left + 1)..right {
        frame.buffer_mut().set_string(x, top, "─", style);
    }
    frame.buffer_mut().set_string(right, top, "┐", style);
    // Left/right edges spanning the content row down to the separator line.
    for y in (top + 1)..=bottom {
        frame.buffer_mut().set_string(left, y, "│", style);
        frame.buffer_mut().set_string(right, y, "│", style);
    }
    Some((left, right))
}

/// Render the result table body directly into `area` (no own Block/borders).
/// The outer Block with title is created by the caller (`render`).
///
/// Layout (matching original dbm):
///   Header: 2 lines (name + type label) + 1 separator = 3 rows total
///   Each data row: 1 content line + 1 separator = 2 rows total
///   Column borders: │ character between columns
///
/// After computing the auto-adjusted h_scroll / v_scroll that keeps the cursor
/// anchored inside the viewport, the values are synced back into `state` so
/// the next frame starts from the correct scroll position (fixes the stale
/// h_scroll problem where state.h_scroll was never updated from the view).
/// Style for an edit-session change marker / text, using the theme's three
/// dirty colours: green for an insert, red for a delete, and the orange for a
/// modified/update row (the same colour the detail draft's diff uses).
fn change_kind_style(p: &crate::common::view::theme::Palette, kind: RowChangeKind) -> Style {
    use crate::common::view::theme::{DIRTY_DELETE_COLOR, DIRTY_INSERT_COLOR};
    Style::default().fg(match kind {
        RowChangeKind::Insert => DIRTY_INSERT_COLOR,
        RowChangeKind::Delete => DIRTY_DELETE_COLOR,
        RowChangeKind::Update => p.dirty,
        RowChangeKind::NoChange => p.fg,
    })
}

fn render_table(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ListState,
    _focused: bool,
    col_resize: Option<usize>,
    active_scrollbar: Option<ActiveScrollbar>,
) {
    // Extract all needed values first to avoid borrow conflicts.
    let result = match state.result.as_ref() {
        Some(r) => r,
        None => return,
    };
    let col_widths = &state.col_widths;
    let state_row = state.row;
    let state_col = state.col;
    let state_selected = state.selected;
    let p = theme.palette();

    // Search highlight inputs: derived once so the cell loop stays flat.
    let search_query = if state.search.query.trim().is_empty() {
        None
    } else {
        Some(state.search.query.as_str())
    };
    let search_matches = &state.search_matches;
    let current_match = state.search_matches.get(state.search_match_index).copied();

    if result.columns.is_empty() {
        let affected = result.rows_affected;
        let text = match affected {
            Some(n) => format!("{n} rows affected"),
            None => "Query completed".to_string(),
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(text, Style::default().fg(p.fg)))),
            area,
        );
        return;
    }

    let row_count = result.rows.len();
    let num_cols = result.columns.len();

    // While an edit session is active the table gains a pinned left gutter
    // column holding each row's change marker (`~`/`+`/`-`), matching the
    // original dbm. It is a constant offset applied at the *screen x* layer, so
    // `h_scroll` keeps its pure column-space meaning.
    let gutter_w = super::layout::results_gutter_width(state);

    // Compute actual table content width to detect horizontal overflow.
    let table_width =
        crate::common::view::format::results_table_width(col_widths).saturating_add(gutter_w);

    // ---- SHARED VIEWPORT CALCULATION ----
    // One source of truth: both render and cell_hit_at call this exact
    // function so the anchored v_scroll / h_scroll / visible_data_rows
    // always match between what we draw and what we hit-test.
    let vs = match compute_viewport_scroll(area, state, row_count, col_widths, table_width as usize)
    {
        Some(v) => v,
        None => return,
    };

    let layout = &vs.layout;
    let table_area = layout.content_area;

    // The marker gutter is pinned at the *screen* left of the table: it
    // consumes one column of the visible area, so the column-space window must
    // shrink by the same amount while it is shown. Without this a table whose
    // columns exactly fill the viewport would paint its rightmost column one
    // cell past the content area (under the vertical scrollbar) and push its
    // truncated `…` text out of view. Cell text is truncated against `text_w`
    // per column, so the narrower window just re-truncates with its own `…`.
    let col_viewport_w = table_area.width.saturating_sub(gutter_w).max(1);

    // Visible content width: when columns don't fill the viewport, avoid
    // rendering empty space beyond the last column (matching original dbm).
    let content_width = table_width.min(table_area.width);

    let h_scroll = vs.h_scroll as u16;
    let v_scroll = vs.v_scroll;
    let visible_data_rows = vs.visible_data_rows;

    // Row separator style (subtle grid line).
    let grid_style = Style::default().fg(p.muted);

    // Root-cause invariant for the whole table body: every visible cell must be
    // written by the current frame, or it retains the previous frame's content.
    // Headers and rows are painted per-column/per-cell as Paragraphs, so the
    // 1-char strip a highlighted column vacates when `h_scroll` shifts — or a
    // row vacates when its selection is cleared while scrolled — would otherwise keep
    // its selection background as a smear. Clearing the entire body region over
    // the content width up front guarantees these cells default to the pane
    // background each frame. The selected-row block and column texts then paint
    // on top, so a transition to unselected/cleared can never leave residue.
    let body_height = (RESULTS_HEADER_HEIGHT
        + (visible_data_rows as u16).saturating_mul(RESULTS_ROW_HEIGHT))
    .min(table_area.height);
    frame.render_widget(
        ratatui::widgets::Clear,
        Rect::new(table_area.x, table_area.y, content_width, body_height),
    );

    // ---- HEADER (3 lines) ----
    // Line 0: column names (bold)
    // Line 1: type labels (green)
    // Line 2: separator
    for col in 0..num_cols {
        let Some(meta) = result.columns.get(col) else {
            break;
        };
        let Some(tv) = results_col_text_view(col, col_widths, col_viewport_w, h_scroll) else {
            continue;
        };
        if tv.text_w == 0 {
            continue;
        }

        // Column's screen x = gutter + text_vis_left − h_scroll (matching the
        // original dbm, plus the edit-mode change gutter).
        let col_x = table_area
            .x
            .saturating_add(gutter_w)
            .saturating_add(
                crate::common::view::format::col_x_start(col, col_widths) as u16
                    + tv.table_text_skip,
            )
            .saturating_sub(h_scroll);

        // Column name (bold).
        let name_style = if col == state_col && state_selected {
            Style::default()
                .fg(p.selection_text)
                .bg(p.selection_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg).add_modifier(Modifier::BOLD)
        };
        let name = crate::common::view::format::truncate_cell_display_from(
            &meta.name,
            tv.table_text_skip,
            tv.text_w,
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(name, name_style))),
            Rect::new(col_x, table_area.y, tv.text_w, 1),
        );

        // Type label: always green on the normal background. It stays in its
        // semantic color rather than taking the selection background — the
        // column *name* is the single visual channel that marks the selected
        // column, so hovering/resizing the header never over-loads a second
        // highlighted row.
        let type_label = column_type_label(meta);
        let type_style = Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD);
        let type_text = crate::common::view::format::truncate_cell_display_from(
            &type_label,
            tv.table_text_skip,
            tv.text_w,
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(type_text, type_style))),
            Rect::new(col_x, table_area.y + 1, tv.text_w, 1),
        );

        // Column border (│) at the right edge of this column. When the mouse
        // hovers / drags this column's right-edge splitter, the border is
        // highlighted (accent) so the resizable region is visible — the theme
        // is the only source of color.
        let col_right = crate::common::view::format::col_x_end(col, col_widths);
        let border_x = table_area
            .x
            .saturating_add(gutter_w)
            .saturating_add(col_right as u16)
            .saturating_sub(h_scroll)
            .saturating_sub(1);
        if border_x >= table_area.x && border_x < table_area.x + table_area.width {
            // The resizable boundary highlight reuses the pane splitters' hover
            // color so hover feedback is one shared source.
            let border_style = if Some(col) == col_resize {
                crate::common::view::splitter::SPLITTER_LINE_HOVER
            } else {
                grid_style
            };
            for y in table_area.y..(table_area.y + RESULTS_HEADER_HEIGHT).min(table_area.bottom()) {
                frame
                    .buffer_mut()
                    .set_string(border_x, y, "│", border_style);
            }
        }
    }

    // Force every header cell to be (re)emitted on each frame. Ratatui stores the
    // trailing column of a wide (CJK) glyph as a blank "hole" whose style is Reset;
    // when a highlighted header scrolls by a single column, that hole compares equal
    // to the previous frame's hole and the diff skips it — but the terminal has
    // physically painted the glyph's right half there, so a 1-cell background smear
    // lingers on the drag trail. Marking the header cells `AlwaysUpdate` re-paints
    // them every frame (wide heads are still emitted with their trailing skipped,
    // so glyphs are never corrupted), which clears residue while remaining O(header).
    {
        use ratatui::buffer::CellDiffOption;
        let hdr_w = content_width.min(table_area.width);
        let hdr_h = RESULTS_HEADER_HEIGHT.min(table_area.height);
        for y in table_area.y..table_area.y + hdr_h {
            for x in table_area.x..table_area.x + hdr_w {
                if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                    cell.set_diff_option(CellDiffOption::AlwaysUpdate);
                }
            }
        }
    }

    // Horizontal separator after header.
    let sep_y = table_area.y + RESULTS_HEADER_HEIGHT - 1;
    if sep_y < table_area.bottom() {
        let sep_right = table_area.x.saturating_add(content_width);
        for x in table_area.x..sep_right.min(table_area.right()) {
            frame.buffer_mut().set_string(x, sep_y, "─", grid_style);
        }
    }

    // ---- DATA ROWS ----
    for vis in 0..visible_data_rows {
        let row_idx = v_scroll + vis;
        if row_idx >= row_count {
            break;
        }
        let y_base = table_area
            .y
            .saturating_add(RESULTS_HEADER_HEIGHT)
            .saturating_add(vis as u16 * RESULTS_ROW_HEIGHT);

        // Row content area — only spans the actual column content width
        // (not the full viewport) to avoid empty-column appearance.
        let row_content_area = Rect::new(
            table_area.x,
            y_base,
            content_width,
            RESULTS_ROW_CONTENT_HEIGHT,
        );

        // Row background (highlight if selected).
        let row_selected = row_idx == state_row && state_selected;
        if row_selected {
            frame.render_widget(
                ratatui::widgets::Block::default().style(Style::default().bg(p.selection_bg)),
                row_content_area,
            );
        }

        // Edit-session change marker in the pinned left gutter: `~` for a row
        // with modified cells, `+` for a pending insert, `-` for a deleted row
        // (matching the original dbm's `gutter_glyph`).
        let row_kind = crate::features::sql_workspace::sql_tab::results::edit::row_change_kind(
            &state.edit,
            row_idx,
        );
        if state.edit.editing {
            let glyph =
                crate::features::sql_workspace::sql_tab::results::edit::gutter_glyph(row_kind);
            if glyph != ' ' {
                let style = change_kind_style(p, row_kind);
                frame
                    .buffer_mut()
                    .set_string(table_area.x, y_base, glyph.to_string(), style);
            }
        }
        // Horizontal span (`(left, right)`) of the current-match cell on this
        // row, if any; the row separator below tints this segment as the cell's
        // bottom border so all four edges are drawn consistently.
        let mut match_bottom_span: Option<(u16, u16)> = None;

        // Draw each cell.
        for col in 0..num_cols {
            let Some(tv) = results_col_text_view(col, col_widths, table_area.width, h_scroll)
            else {
                continue;
            };
            if tv.text_w == 0 {
                continue;
            }

            let value = result
                .rows
                .get(row_idx)
                .and_then(|r| r.get(col))
                .map(String::as_str)
                .unwrap_or("");

            let is_active = state_col == col && state_selected;
            let cell_selected = row_selected && is_active;
            // Edit-session tint: a *modified* row only colors the cells that
            // actually changed (so the diff is readable), while an inserted or
            // deleted row is tinted as a whole — matching the original dbm.
            let dirty = gutter_w > 0
                && (row_kind == RowChangeKind::Insert
                    || row_kind == RowChangeKind::Delete
                    || crate::features::sql_workspace::sql_tab::results::edit::cell_is_dirty(
                        &state.edit,
                        row_idx,
                        col,
                    ));
            // Dirty text keeps its change-kind colour (insert green / delete
            // red / update orange) even on the selection background, so a
            // focused dirty row still reads as changed.
            let kind_fg = dirty.then(|| change_kind_style(p, row_kind).fg.unwrap_or(p.fg));
            let base_style = if cell_selected {
                Style::default()
                    .fg(kind_fg.unwrap_or(p.selection_focus_text))
                    .bg(p.selection_cell_bg)
                    .add_modifier(Modifier::BOLD)
            } else if row_selected || is_active {
                Style::default()
                    .fg(kind_fg.unwrap_or(p.selection_text))
                    .bg(p.selection_bg)
            } else {
                Style::default().fg(kind_fg.unwrap_or(p.fg))
            };

            // Shift the window for the cell holding the current match so the highlighted
            // query stays in view — including when the focus has moved to another
            // cell, so the viewport stays aligned with the offset/length read-out
            // (which still describes the current match). It is derived at render
            // time and never cached (stale skips on other cells are impossible).
            let is_match_cell = current_match.is_some_and(|m| m.row == row_idx && m.col == col);
            let text_skip = if is_match_cell {
                let m = current_match.unwrap();
                let q = search_query.unwrap_or("");
                tv.table_text_skip.saturating_add(match_cell_text_skip(
                    value,
                    m,
                    q,
                    tv.text_w,
                    tv.table_text_skip,
                ))
            } else {
                tv.table_text_skip
            };
            let col_x = table_area
                .x
                .saturating_add(gutter_w)
                .saturating_add(
                    crate::common::view::format::col_x_start(col, col_widths) as u16
                        + tv.table_text_skip,
                )
                .saturating_sub(h_scroll);

            let highlight_line = search_query.and_then(|q| {
                let starts = super::search::match_starts_in_cell(search_matches, row_idx, col);
                if starts.is_empty() {
                    None
                } else {
                    Some((q, starts))
                }
            });
            // Matched text uses a uniform accent style so it reads against the
            // pane background. On a selected row/column the accent fg would
            // clash with the selection background, so we instead fill each hit
            // with the accent *background* (the same background as
            // `current_match_style`), keeping the selection foreground bold so
            // matches stay visible inside the crosshair region. The *current*
            // match cell is distinguished solely by its frame
            // (`match_cell_border_style`), so both highlight args below use the
            // same text style.
            let match_text_style = if row_selected || is_active {
                base_style.bg(p.accent)
            } else {
                p.match_style()
            };
            let line = if let Some((q, starts)) = highlight_line {
                super::search::cell_highlight_line(
                    value,
                    text_skip,
                    tv.text_w,
                    q,
                    &starts,
                    current_match
                        .filter(|m| m.row == row_idx && m.col == col)
                        .map(|m| m.start),
                    base_style,
                    match_text_style,
                    match_text_style,
                )
            } else {
                Line::from(Span::styled(
                    crate::common::view::format::truncate_cell_display_from(
                        value, text_skip, tv.text_w,
                    ),
                    base_style,
                ))
            };
            frame.render_widget(
                Paragraph::new(line),
                Rect::new(col_x, y_base, tv.text_w, RESULTS_ROW_CONTENT_HEIGHT),
            );

            // Column border for this row: always the plain grid line — dirty
            // rows are marked by the gutter glyph and the cell text colour,
            // never by a border colour.
            let col_right = crate::common::view::format::col_x_end(col, col_widths);
            let border_x = table_area
                .x
                .saturating_add(gutter_w)
                .saturating_add(col_right as u16)
                .saturating_sub(h_scroll)
                .saturating_sub(1);
            if border_x >= table_area.x && border_x < table_area.x + table_area.width {
                for y in y_base..(y_base + RESULTS_ROW_HEIGHT).min(table_area.bottom()) {
                    frame.buffer_mut().set_string(border_x, y, "│", grid_style);
                }
            }

            // Recolor the surrounding grid border into an accent frame for the
            // current-match cell so it stands out (drawn after the plain border).
            // The bottom edge is tinted later by the row separator.
            if is_match_cell {
                match_bottom_span = draw_match_cell_frame(
                    frame,
                    current_match_border_style(p),
                    table_area,
                    col_x,
                    y_base,
                    border_x,
                );
            }
        }

        // Row separator line (tinted as the current-match cell's bottom border).
        let row_sep_y = y_base + RESULTS_ROW_CONTENT_HEIGHT;
        if row_sep_y < table_area.bottom() {
            let accent = current_match_border_style(p);
            let sep_right = table_area.x.saturating_add(content_width);
            for x in table_area.x..sep_right.min(table_area.right()) {
                let (glyph, style) = match match_bottom_span {
                    Some((l, r)) if x >= l && x <= r => {
                        let c = if x == l {
                            "└"
                        } else if x == r {
                            "┘"
                        } else {
                            "─"
                        };
                        (c, accent)
                    }
                    _ => ("─", grid_style),
                };
                frame.buffer_mut().set_string(x, row_sep_y, glyph, style);
            }
        }
    }

    // Vertical scrollbar for the table rows.
    if let Some(bar) = layout.v_scrollbar {
        crate::common::view::pane_scrollbar::draw_vertical_pane_scrollbar(
            frame,
            bar,
            v_scroll,
            visible_data_rows,
            vs.max_v_scroll,
            p,
            matches!(active_scrollbar, Some(ActiveScrollbar::ResultsV)),
        );
    }

    // Horizontal scrollbar for columns that overflow the viewport.
    if let Some(bar) = layout.h_scrollbar {
        crate::common::view::pane_scrollbar::draw_horizontal_pane_scrollbar(
            frame,
            bar,
            h_scroll as usize,
            table_area.width as usize,
            vs.max_h_scroll,
            p,
            matches!(active_scrollbar, Some(ActiveScrollbar::ResultsH)),
        );
    }

    // Sync computed scroll values and viewport info back to state so the next
    // frame starts from the correct position (fixes stale h_scroll issue).
    // Uses Cell for interior mutability — allows writing through &ListState.
    state.h_scroll.set(vs.h_scroll);
    state.v_scroll.set(v_scroll);
    state.viewport_width.set(table_area.width);
    state.viewport_rows.set(visible_data_rows);
}

/// The target width for column `col` given a drag pointer `x`, computed over
/// the same shared geometry as [`col_resize_hit_at`]. The caller clamps the
/// final value via the update message.
pub fn col_width_from_drag_x(list_area: Rect, state: &ListState, col: usize, x: u16) -> u16 {
    let (_table_area, content_area, h_scroll) = match results_geometry(list_area, state) {
        Some(g) => g,
        None => return crate::common::view::format::DEFAULT_RESULTS_COL_WIDTH,
    };
    // Subtract the edit gutter: columns start after it, so a drag x maps into
    // column space only once the pinned marker column is removed.
    let gutter_w = super::layout::results_gutter_width(state);
    let rel_x = x.saturating_sub(content_area.x).saturating_sub(gutter_w) as usize + h_scroll;
    let start = crate::common::view::format::col_x_start(col, &state.col_widths);
    rel_x.saturating_sub(start) as u16
}

#[cfg(test)]
mod tests {
    use crate::common::view::theme;
    use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;
    use crate::features::sql_workspace::sql_tab::results::state::QueryResultData;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;

    fn sample_result() -> QueryResultData {
        QueryResultData {
            columns: (0..6)
                .map(|i| ColumnInfo {
                    name: format!("field{i}name"),
                    type_name: "int4".into(),
                    type_display: "int4".into(),
                    comment: None,
                })
                .collect(),
            rows: vec![vec![
                "0".into(),
                "1".into(),
                "2".into(),
                "3".into(),
                "4".into(),
                "5".into(),
            ]],
            rows_affected: None,
            total_rows: Some(1),
        }
    }

    fn make_state(h_scroll: usize, col: usize) -> super::ListState {
        let mut s = super::ListState::new();
        s.result = Some(sample_result());
        // Force horizontal overflow: six 40-wide columns in a narrow viewport.
        s.col_widths = vec![40; 6];
        s.row = 0;
        s.col = col;
        s.selected = true;
        s.scroll_locked.set(true); // keep the manually-set h_scroll
        s.h_scroll.set(h_scroll);
        s
    }

    /// Reproduce the header-highlight smear: drawing an earlier h_scroll frame
    /// then a shifted one on the SAME terminal must leave no residue — of any
    /// glyph or color — beyond what a fresh single render of the shifted frame
    /// shows.
    #[test]
    fn horizontal_scroll_leaves_no_header_selection_residue() {
        let area = Rect::new(0, 0, 30, 10);
        let (from, to, col) = (0usize, 45usize, 2usize);

        // Baseline: a fresh terminal rendered directly at the destination h_scroll.
        let mut fresh = Terminal::new(TestBackend::new(30, 10)).unwrap();
        fresh
            .draw(|f| {
                super::render(
                    f,
                    &theme::default(),
                    area,
                    &make_state(to, col),
                    true,
                    None,
                    None,
                )
            })
            .unwrap();
        let fresh_buf = fresh.backend().buffer().clone();

        // Cumulative: step through intermediate h_scroll values on the SAME
        // terminal (mirroring a real drag), ending at the destination.
        let mut cumul = Terminal::new(TestBackend::new(30, 10)).unwrap();
        for h in (from..=to).step_by(3) {
            cumul
                .draw(|f| {
                    super::render(
                        f,
                        &theme::default(),
                        area,
                        &make_state(h, col),
                        true,
                        None,
                        None,
                    )
                })
                .unwrap();
        }

        assert_eq!(
            cumul.backend().buffer().content,
            fresh_buf.content,
            "stale screen content (glyph or color) left after h_scroll moves the highlight"
        );
    }
}
