//! Results feature rendering: composes the `list`, `detail`, and `splitter`
//! sub-feature views.
//!
//! A single outer Block with border + title wraps everything. When the detail
//! is visible the Block's inner area splits horizontally into
//! `[list | splitter | detail]`; otherwise the list fills the whole inner area.

use ratatui::Frame;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::common::components::search::{pane_search_bottom_title_line, pane_search_label_line};
use crate::common::layout::pane_scrollbar::ActiveScrollbar;
use crate::common::view::hints::{draw_pane_footer, results_pane_footer_text};
use crate::common::view::theme::Theme;

use super::detail::view as detail_view;
use super::layout::compute_results_layout;
use super::list::view as list_view;
use super::pagination::{layout_pagination_bar, pagination_toolbar_line};
use super::splitter::view as splitter_view;
use super::state::ResultsState;

/// Footer notice shown when a leave was blocked by unsaved edits in the list's
/// edit session (commit or roll back first). Drawn in the failure colour, like
/// the detail draft's and the connections form's dirty-leave notices.
pub const RESULTS_EDIT_LEAVE_WARNING: &str =
    "Unsaved edits — commit (C-s) or roll back (C-u) before leaving";

/// Render the Results feature. The single outer Block + title wraps the
/// entire `area` (both list and, when `detail_open`, the detail preview).
#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: ratatui::layout::Rect,
    state: &ResultsState,
    focused: bool,
    splitter_hover: bool,
    splitter_drag: bool,
    col_resize: Option<usize>,
    active_scrollbar: Option<ActiveScrollbar>,
) -> Option<crate::common::editor::EditorMouseHitArea> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let p = theme.palette();

    let title = pane_search_label_line(
        " [R] Results",
        focused,
        false,
        Style::default().fg(p.muted),
        None,
        p.search_active_style(),
    );

    // The pane search `/query [n/m]` renders on the bottom border
    // (`title_bottom`), matching the original dbm's search placement. When the
    // search is visible, append the scope / count / offset read-out.
    let search_extra = state.list.search_title_extra();
    let search_title = pane_search_bottom_title_line(
        &state.list.search,
        state.list.row,
        state.list.row_count(),
        Some(area.width.saturating_sub(2)),
        state
            .list
            .search
            .is_visible()
            .then_some(search_extra.as_str()),
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
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Single source of truth for the whole Results layout: the pagination
    // toolbar and the list footer span the full inner width (both the table and
    // the detail), and only the content band above them is narrowed to the list
    // side by the horizontal list|detail split. This keeps the toolbar and
    // footer width constant whether or not the detail is open.
    let sql_status = state.list.executed_sql_display();
    let layout = compute_results_layout(
        inner,
        state.detail_open,
        state.splitter.detail_pane_width,
        state.list.row_count(),
        state.list.search.text_input_active(),
        &sql_status,
    );

    list_view::render(
        frame,
        theme,
        layout.list,
        &state.list,
        focused,
        col_resize,
        active_scrollbar,
    );

    if let Some(rect) = layout.splitter {
        splitter_view::render(frame, rect, splitter_hover, splitter_drag);
    }

    // The detail preview sits inside the content band, so its bottom aligns
    // with the table's last row (both stop where the toolbar begins). A focused
    // cell editor hands back its hit region so clicks map onto the draft.
    let mut detail_hit: Option<crate::common::editor::EditorMouseHitArea> = None;
    if let (Some(detail_area), true) = (layout.detail, state.detail_open) {
        let body = state.list.selected_cell().unwrap_or_default();
        let col_name = state.list.selected_column_name().unwrap_or("").to_string();
        let title_text = format!(
            " [{}] row {}",
            if col_name.is_empty() { "?" } else { &col_name },
            state.list.row + 1
        );
        let edit_editing = state.list.edit.editing;
        detail_hit = detail_view::render(
            frame,
            theme,
            detail_area,
            &state.detail,
            &body,
            title_text,
            edit_editing,
            focused,
        );
    }

    // Full-width pagination toolbar (spans both the table and the detail).
    // The text itself is right-aligned: it renders into the right-anchored
    // `bar_rect` computed by `layout_pagination_bar` (the same rect the mouse
    // hit-testing uses), matching the original dbm.
    if let Some(pag_area) = layout.pagination {
        let row_limit = state.list.row_limit;
        let page = state.list.page;
        let total_rows = state
            .list
            .result
            .as_ref()
            .map(|r| r.total_rows)
            .unwrap_or_default();
        let row_count = state.list.row_count();
        let (counting, show_count) = state.list.toolbar_count_flags();
        let bar = layout_pagination_bar(
            pag_area, row_limit, page, total_rows, row_count, counting, show_count,
        );
        let toolbar = pagination_toolbar_line(
            row_limit,
            page,
            total_rows,
            row_count,
            counting,
            show_count,
            Style::default().fg(p.accent),
            Style::default().fg(p.muted),
        );
        frame.render_widget(Paragraph::new(toolbar), bar.bar_rect);
    }

    // Full-width list footer (spans both the table and the detail). While a
    // blocked leave left unsaved edits on the table, the footer shows the
    // interception reason in the failure colour (connections / detail style).
    let list_leave_blocked =
        state.list.leave_warning && state.list.edit.editing && state.list.edit.is_dirty();
    if list_leave_blocked {
        let style = Style::default().fg(Color::Red);
        let lines: Vec<Line> = RESULTS_EDIT_LEAVE_WARNING
            .split('\n')
            .map(|l| Line::from(Span::styled(l.to_string(), style)))
            .collect();
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }),
            layout.footer,
        );
    } else {
        let hint = results_pane_footer_text(state.list.search.text_input_active(), &sql_status);
        draw_pane_footer(frame, theme, layout.footer, &hint);
    }
    detail_hit
}
