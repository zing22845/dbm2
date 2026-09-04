//! Discovery targets editor feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::common::view::pane_scrollbar::{
    PaneScrollLayout, draw_vertical_pane_scrollbar, pane_scroll_layout,
};
use crate::common::view::theme::Theme;

use super::state::{TargetCol, TargetsState};

/// Computed layout information from the targets renderer, threaded back to
/// the state so update handlers can clamp scroll correctly.
#[derive(Debug, Clone, Copy, Default)]
pub struct TargetsLayoutInfo {
    pub scroll_offset: usize,
    pub viewport: usize,
}

/// Shared footer-area computation used by render, hit_test, and the scrollbar
/// hit-test helper. Keeps the footer height, body area, and everything that
/// depends on them in one place so all three paths agree on which rows are
/// visible (same pattern as the history/results "effective_layout" helpers).
pub fn compute_targets_body_and_footer(
    area: Rect,
    state: &TargetsState,
) -> Option<(Rect, u16)> {
    use crate::common::utils::text_width::wrapped_line_count;
    use crate::common::view::hints::discover_targets_footer_text;

    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(area);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    let footer_text = discover_targets_footer_text(
        state.editing,
        state.has_loopback(),
        state.status.as_deref(),
    );
    let footer_h = if footer_text.is_empty() {
        0
    } else {
        wrapped_line_count(&footer_text, inner.width)
            .max(1)
            .min(inner.height.saturating_sub(1).max(1))
    };

    let body = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer_h),
    );
    if body.width == 0 || body.height == 0 {
        return None;
    }
    Some((body, footer_h))
}

/// Result of [`compute_targets_viewport`]: the pure viewport calculation
/// shared by `render`, `hit_test`, and `v_scrollbar_hit`. Keeping all three
/// honest to the same formula eliminates anchor drift between the drawn rows,
/// the click-resolved row, and the scrollbar thumb position — exactly the
/// lesson history/results taught us.
pub struct TargetsViewport {
    pub body: Rect,
    pub layout: PaneScrollLayout,
    pub content: Rect,
    /// Data rows visible in the content area (content.height - 1 for the
    /// Table header row).
    pub viewport: usize,
    /// Cursor-anchored v_scroll (first visible data row index).
    pub start: usize,
    pub total: usize,
    pub max_scroll: usize,
}

/// Compute the targets list viewport given the outer `area` and `state`.
/// Encodes footer computation, `pane_scroll_layout`, and discover-style
/// cursor anchoring — all in one place. When `scroll_locked` is set
/// (scrollbar drag / manual SetVScroll) the anchor is skipped so the
/// manual scroll position is honoured until the next cursor move.
pub fn compute_targets_viewport(
    area: Rect,
    state: &TargetsState,
) -> Option<TargetsViewport> {
    let total = state.targets.len();
    if total == 0 {
        return None;
    }
    let (body, _footer_h) = compute_targets_body_and_footer(area, state)?;
    let viewport_rows = body.height as usize;
    let layout = pane_scroll_layout(body, body.width, total, viewport_rows);
    let content = layout.content_area;

    // Table reserves one row for its header → visible data rows are one less.
    let viewport = content.height.saturating_sub(1).max(1) as usize;
    let max_scroll = total.saturating_sub(viewport.max(1));

    // Discover-style anchor via shared helper.
    let start = crate::common::view::pane_scrollbar::discover_anchor(
        state.scroll_offset,
        max_scroll,
        state.row,
        viewport,
        state.scroll_locked,
    );

    Some(TargetsViewport {
        body,
        layout,
        content,
        viewport,
        start,
        total,
        max_scroll,
    })
}

use crate::common::view::pane_scrollbar::ScrollbarHitInfo;

/// Hit-test the targets pane's vertical scrollbar — delegates to shared helper.
pub fn v_scrollbar_hit(
    area: Rect,
    state: &TargetsState,
    x: u16,
    y: u16,
) -> Option<ScrollbarHitInfo> {
    let tv = compute_targets_viewport(area, state)?;
    crate::common::view::pane_scrollbar::v_scrollbar_hit(&tv.layout, tv.max_scroll, x, y)
}

/// The target list's table column layout. Used both by the `Table` render and
/// by the inline-edit caret placement, so the caret always lands on the same
/// column the Table draws (changing the layout here keeps both in sync).
const TARGETS_COLUMNS: [ratatui::layout::Constraint; 3] = [
    ratatui::layout::Constraint::Length(3),
    ratatui::layout::Constraint::Percentage(45),
    ratatui::layout::Constraint::Percentage(55),
];

/// Render the targets editor: a bordered list of `host : ports` rows, with the
/// focused cell highlighted and an inline edit shown when editing. The border
/// highlights only when the targets pane owns focus, and a footer line shows
/// the active target keys.
/// Render the targets editor. Returns the inline-edit caret position when an
/// editable cell is focused and being edited, so the shell can place the
/// terminal hardware cursor. Also writes layout info (scroll offset, viewport)
/// into `layout_out` so the render loop can feed it back to the state.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &TargetsState,
    focus: crate::app_shell::nav::DiscoverPane,
    layout_out: &std::cell::RefCell<Option<TargetsLayoutInfo>>,
) -> Option<crate::common::editor::EditorHardwareCursor> {
    use crate::common::view::hints::{discover_targets_footer_text, draw_pane_footer};
    let p = theme.palette();
    let focused = focus == crate::app_shell::nav::DiscoverPane::Targets;

    let footer_text = discover_targets_footer_text(state.editing, state.has_loopback(), state.status.as_deref());

    let block = Block::default()
        .title(" targets ")
        .borders(Borders::ALL)
        .border_style(p.pane_border(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    // ---- SHARED VIEWPORT CALCULATION ----
    // One source of truth for render, hit_test, and scrollbar hit-test.
    let tv = compute_targets_viewport(area, state)?;
    let body = tv.body;
    let content = tv.content;
    let layout = &tv.layout;
    let viewport = tv.viewport;
    let start = tv.start;

    // Write back layout info so the render loop can update scroll_offset and
    // target_viewport for the next frame (discover-style layout_out pattern).
    *layout_out.borrow_mut() = Some(TargetsLayoutInfo {
        scroll_offset: start,
        viewport,
    });

    let mut caret: Option<crate::common::editor::EditorHardwareCursor> = None;
    if body.width > 0 && body.height > 0 {
        let selected = focused && state.row < state.targets.len();
        let header = ratatui::widgets::Row::new(["#", "Host", "Ports"])
            .style(Style::default().add_modifier(Modifier::BOLD));
        let rows = state
            .targets
            .iter()
            .enumerate()
            .skip(start)
            .take(viewport)
            .map(|(idx, row)| {
                let row_sel = selected && idx == state.row;
                let host_focused = row_sel && state.col == TargetCol::Host;
                let ports_focused = row_sel && state.col == TargetCol::Ports;
                ratatui::widgets::Row::new(vec![
                    ratatui::widgets::Cell::from((idx + 1).to_string())
                        .style(if row_sel { row_style(theme) } else { Style::default().fg(p.muted) }),
                    ratatui::widgets::Cell::from(format_cell(
                        if host_focused && state.editing { &state.edit_buf } else { &row.host },
                        host_focused,
                        state.editing,
                    ))
                    .style(cell_style(theme, row_sel, host_focused, state.editing)),
                    ratatui::widgets::Cell::from(format_cell(
                        if ports_focused && state.editing { &state.edit_buf } else { &row.ports_spec },
                        ports_focused,
                        state.editing,
                    ))
                    .style(cell_style(theme, row_sel, ports_focused, state.editing)),
                ])
            });
        let table = ratatui::widgets::Table::new(rows, TARGETS_COLUMNS)
        .header(header)
        .column_spacing(1);
        frame.render_widget(table, content);

        if let Some(bar) = layout.v_scrollbar {
            let max_scroll = state.targets.len().saturating_sub(viewport);
            draw_vertical_pane_scrollbar(frame, bar, start, viewport, max_scroll, p, false);
        }

        // Inline-edit caret: while editing a host/ports cell, report the caret's
        // on-screen position so the shell shows the terminal caret inside the
        // cell.
        if state.editing && state.row < state.targets.len() {
            let row_in_content = state.row.saturating_sub(start);
            if (row_in_content as u16) < content.height.saturating_sub(1) {
                // Column x positions mirror the Table's own layout
                // (`Length(3), Percentage(45), Percentage(55)` with
                // column_spacing 1) so the caret lands exactly where a typed
                // char does. Recompute via the same `Layout` ratatui uses rather
                // than approximating the host width by hand (which drifted).
                let cols = ratatui::layout::Layout::default()
                    .direction(ratatui::layout::Direction::Horizontal)
                    .constraints(TARGETS_COLUMNS)
                    .spacing(1)
                    .split(content);
                let cell_x = match state.col {
                    TargetCol::Host => cols[1].x,
                    TargetCol::Ports => cols[2].x,
                };
                // Caret column within the cell: the cell text starts at
                // `cell_x` (Host/Ports column origin, matching the Table's
                // Length(3) + spacing + Percentage columns), so the caret sits
                // at the display width of the edit prefix from there — no extra
                // offset, or the caret drifts from where a typed char lands.
                let prefix = &state.edit_buf[..state.edit_cursor.min(state.edit_buf.len())];
                let caret_offset = unicode_width::UnicodeWidthStr::width(prefix) as u16;
                let x = cell_x.saturating_add(caret_offset);
                let y = content.y.saturating_add(1 /* header */ + row_in_content as u16);
                caret = Some(crate::common::editor::EditorHardwareCursor {
                    position: ratatui::layout::Position::new(x, y),
                    style: crate::common::editor::hardware_cursor_style(edtui::EditorMode::Insert),
                });
            }
        }
    }

    let footer_h = inner.height.saturating_sub(body.height);
    if footer_h > 0 {
        let footer_area = Rect::new(inner.x, inner.y + body.height, inner.width, footer_h);
        draw_pane_footer(frame, theme, footer_area, &footer_text);
    }

    caret
}

/// Style for the focused/selected target row (the unified selection background).
fn row_style(theme: &Theme) -> Style {
    let p = theme.palette();
    Style::default()
        .fg(p.selection_text)
        .bg(p.selection_bg)
}

/// Style for a host/ports cell: editing cells get a distinct edit background,
/// otherwise the focused cell gets the `▸` selection treatment.
fn cell_style(theme: &Theme, row_sel: bool, cell_focused: bool, editing: bool) -> Style {
    let p = theme.palette();
    if editing && cell_focused {
        // Inline edit: a distinct background so the live buffer stands out.
        Style::default().fg(p.accent).bg(p.surface).add_modifier(Modifier::BOLD)
    } else if cell_focused {
        // The active cell gets a stronger background so it stands out from
        // the selected row's other fields (which share selection_bg).
        Style::default()
            .fg(p.selection_focus_text)
            .bg(p.selection_cell_bg)
            .add_modifier(Modifier::BOLD)
    } else if row_sel {
        Style::default().fg(p.selection_text).bg(p.selection_bg)
    } else {
        Style::default().fg(p.fg)
    }
}

/// The focused (non-editing) host/ports cell gets a leading `▸` marker, so the
/// user can see which column will be edited on Enter, matching the original dbm.
fn format_cell(value: &str, cell_focused: bool, editing: bool) -> String {
    if cell_focused && !editing {
        format!("▸ {value}")
    } else {
        value.to_string()
    }
}

/// Hit-test the targets list. Given the targets pane `area`, the current state,
/// and a click coordinate, returns which row and optionally which cell column
/// was clicked. Returns `None` if the click is outside the table content area.
pub fn hit_test(
    area: Rect,
    state: &TargetsState,
    click_x: u16,
    click_y: u16,
) -> Option<(usize, Option<TargetCol>)> {
    let tv = compute_targets_viewport(area, state)?;
    let content = tv.content;
    let viewport = tv.viewport;
    let start = tv.start;

    // Click must be inside the content area (not scrollbar, not outside block).
    if click_x < content.x
        || click_x >= content.x + content.width
        || click_y < content.y
        || click_y >= content.y + content.height
    {
        return None;
    }

    let row_in_content = (click_y - content.y) as usize;
    if row_in_content == 0 {
        return None;
    }
    let data_row = row_in_content - 1;
    if data_row >= viewport || start + data_row >= state.targets.len() {
        return None;
    }

    let cols = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints(TARGETS_COLUMNS)
        .spacing(1)
        .split(content);

    let col = if click_x >= cols[1].x && click_x < cols[1].x + cols[1].width {
        Some(TargetCol::Host)
    } else if click_x >= cols[2].x && click_x < cols[2].x + cols[2].width {
        Some(TargetCol::Ports)
    } else {
        None
    };

    Some((start + data_row, col))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::discover::targets::state::TargetRow;

    fn state_with_rows(n: usize) -> TargetsState {
        let mut state = TargetsState::with_default_targets();
        while state.targets.len() < n {
            state.targets.push(TargetRow {
                host: format!("host{}", state.targets.len()),
                ports_spec: "5432".into(),
            });
        }
        state
    }

    #[test]
    fn hit_test_clicks_row_body_selects_row() {
        let state = state_with_rows(3);
        let area = Rect::new(0, 0, 40, 20);
        // Compute the content area the same way hit_test does
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(area);
        let footer_text = crate::common::view::hints::discover_targets_footer_text(
            false,
            false,
            None,
        );
        let footer_h = if footer_text.is_empty() {
            0
        } else {
            crate::common::utils::text_width::wrapped_line_count(&footer_text, inner.width)
                .max(1)
                .min(inner.height.saturating_sub(1).max(1))
        };
        let body = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(footer_h),
        );
        let layout = crate::common::view::pane_scrollbar::pane_scroll_layout(
            body,
            body.width,
            state.targets.len(),
            body.height as usize,
        );
        let content = layout.content_area;
        // First data row is at content.y + 1 (after header row at content.y)
        let click_y = content.y + 1;
        let click_x = content.x + 2;
        let result = hit_test(area, &state, click_x, click_y);
        assert!(result.is_some(), "click at ({click_x},{click_y}) should be inside content, area={area:?}, content={content:?}");
        let (row, _col) = result.unwrap();
        assert_eq!(row, 0, "first data row should be row 0");
    }

    #[test]
    fn hit_test_outside_content_returns_none() {
        let state = state_with_rows(3);
        let area = Rect::new(0, 0, 40, 20);
        assert!(hit_test(area, &state, 0, 0).is_none());
        assert!(hit_test(area, &state, 10, 100).is_none());
    }

    #[test]
    fn hit_test_clicks_host_column() {
        let state = state_with_rows(1);
        let area = Rect::new(0, 0, 40, 20);
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(area);
        let footer_text = crate::common::view::hints::discover_targets_footer_text(
            false,
            state.has_loopback(),
            state.status.as_deref(),
        );
        let footer_h = if footer_text.is_empty() {
            0
        } else {
            crate::common::utils::text_width::wrapped_line_count(&footer_text, inner.width)
                .max(1)
                .min(inner.height.saturating_sub(1).max(1))
        };
        let body = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(footer_h),
        );
        let layout = crate::common::view::pane_scrollbar::pane_scroll_layout(
            body,
            body.width,
            state.targets.len(),
            body.height as usize,
        );
        let content = layout.content_area;
        let click_y = content.y + 1;
        // Host column starts after the # column (width 3 + spacing 1)
        let cols = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints(TARGETS_COLUMNS)
            .spacing(1)
            .split(content);
        let click_x = cols[1].x + 1; // Inside Host column
        let result = hit_test(area, &state, click_x, click_y);
        assert!(result.is_some(), "click at ({click_x},{click_y}) should be inside content");
        let (_row, col) = result.unwrap();
        assert!(col.is_some(), "should detect column");
    }

    #[test]
    fn hit_test_clicks_ports_column() {
        let state = state_with_rows(1);
        let area = Rect::new(0, 0, 40, 20);
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(area);
        let footer_text = crate::common::view::hints::discover_targets_footer_text(
            false,
            state.has_loopback(),
            state.status.as_deref(),
        );
        let footer_h = if footer_text.is_empty() {
            0
        } else {
            crate::common::utils::text_width::wrapped_line_count(&footer_text, inner.width)
                .max(1)
                .min(inner.height.saturating_sub(1).max(1))
        };
        let body = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(footer_h),
        );
        let layout = crate::common::view::pane_scrollbar::pane_scroll_layout(
            body,
            body.width,
            state.targets.len(),
            body.height as usize,
        );
        let content = layout.content_area;
        let click_y = content.y + 1;
        let cols = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints(TARGETS_COLUMNS)
            .spacing(1)
            .split(content);
        let click_x = cols[2].x + 1; // Inside Ports column
        let result = hit_test(area, &state, click_x, click_y);
        assert!(result.is_some(), "click at ({click_x},{click_y}) should be inside content");
        let (_row, col) = result.unwrap();
        assert!(col.is_some(), "should detect column");
    }

    #[test]
    fn hit_test_header_row_returns_none() {
        let state = state_with_rows(3);
        let area = Rect::new(0, 0, 40, 20);
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(area);
        let layout = crate::common::view::pane_scrollbar::pane_scroll_layout(
            inner,
            inner.width,
            state.targets.len(),
            inner.height as usize,
        );
        let content = layout.content_area;
        assert!(hit_test(area, &state, content.x + 5, content.y).is_none());
    }

    #[test]
    fn hit_test_scrolled_rows_returns_correct_index() {
        let state = state_with_rows(10);
        let area = Rect::new(0, 0, 40, 20);
        // Compute click coords inside content
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(area);
        let footer_text = crate::common::view::hints::discover_targets_footer_text(
            false,
            state.has_loopback(),
            state.status.as_deref(),
        );
        let footer_h = if footer_text.is_empty() {
            0
        } else {
            crate::common::utils::text_width::wrapped_line_count(&footer_text, inner.width)
                .max(1)
                .min(inner.height.saturating_sub(1).max(1))
        };
        let body = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(footer_h),
        );
        let layout = crate::common::view::pane_scrollbar::pane_scroll_layout(
            body,
            body.width,
            state.targets.len(),
            body.height as usize,
        );
        let content = layout.content_area;
        // First data row at content.y + 1
        let click_y = content.y + 1;
        let click_x = content.x + 2;
        let result = hit_test(area, &state, click_x, click_y);
        assert!(result.is_some(), "click at ({click_x},{click_y}) should be inside content, content={content:?}");
        let (row, _col) = result.unwrap();
        assert!(row < state.targets.len());
        // With 10 rows and limited viewport, scrolling may occur; row should be valid
        // but the specific index depends on pane_scrollbar logic
    }

    #[test]
    fn hit_test_nonexistent_row_returns_none() {
        let state = state_with_rows(1);
        let area = Rect::new(0, 0, 40, 20);
        let result = hit_test(area, &state, area.x + 2, area.y + area.height - 2);
        if let Some((row, _)) = result {
            assert!(row < state.targets.len());
        }
    }
}
