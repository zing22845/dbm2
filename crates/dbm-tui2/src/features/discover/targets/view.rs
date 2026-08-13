//! Discovery targets editor feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::common::view::pane_scrollbar::{draw_vertical_pane_scrollbar, pane_scroll_layout};
use crate::common::view::theme::Theme;

use super::state::{TargetCol, TargetsState};

/// Render the targets editor: a bordered list of `host : ports` rows, with the
/// focused cell highlighted and an inline edit shown when editing. The border
/// highlights only when the targets pane owns focus, and a footer line shows
/// the active target keys.
/// Render the targets editor. Returns the inline-edit caret position when an
/// editable cell is focused and being edited, so the shell can place the
/// terminal hardware cursor.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &TargetsState,
    focus: crate::app_shell::nav::DiscoverPane,
) -> Option<crate::common::editor::EditorHardwareCursor> {
    use crate::common::utils::text_width::wrapped_line_count;
    use crate::common::view::hints::{discover_targets_footer_text, draw_pane_footer};
    let p = theme.palette();
    let focused = focus == crate::app_shell::nav::DiscoverPane::Targets;

    let footer_text = discover_targets_footer_text(state.editing, state.has_loopback(), state.status.as_deref());

    let block = Block::default()
        .title(" targets ")
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    // The footer may span the hints line plus a loopback note and/or the last
    // paste/undo/redo status line. Size it to the actual wrapped line count at
    // the pane's width (a long loopback note wraps under a narrow pane), but
    // never let it crowd out the whole body.
    let footer_h = if footer_text.is_empty() {
        0
    } else {
        wrapped_line_count(&footer_text, inner.width)
            .max(1)
            .min(inner.height.saturating_sub(1).max(1))
    };

    // Body is the inner area minus the footer strip. Rendered as a table with
    // `#` line-number, `Host` and `Ports` columns (matching the original dbm),
    // so host and ports are independently editable and visible.
    let body = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer_h),
    );
    let mut caret: Option<crate::common::editor::EditorHardwareCursor> = None;
    if body.width > 0 && body.height > 0 {
        let selected = focused && state.row < state.targets.len();
        let header = ratatui::widgets::Row::new(["#", "Host", "Ports"])
            .style(Style::default().add_modifier(Modifier::BOLD));
        // Reserve a vertical scrollbar column when the list overflows its
        // viewport. The scroll offset is derived here from the cursor using
        // edge-scroll (no centering): the window starts at the top and only
        // scrolls once the cursor passes the bottom edge, so the cursor row is
        // always kept within the visible window.
        let viewport_rows = body.height as usize;
        let layout = pane_scroll_layout(body, body.width, state.targets.len(), viewport_rows);
        let content = layout.content_area;
        // A Table reserves one row for its header, so the visible content rows
        // are one less than the area height. Use that as the scroll viewport so
        // the cursor never falls below the last visible data row.
        let viewport = content.height.saturating_sub(1).max(1) as usize;
        let start = if state.row < viewport {
            0
        } else {
            state.row - viewport + 1
        };
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
        let table = ratatui::widgets::Table::new(
            rows,
            [
                ratatui::layout::Constraint::Length(3),
                ratatui::layout::Constraint::Percentage(45),
                ratatui::layout::Constraint::Percentage(55),
            ],
        )
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
                // Column x positions: `#`(3) + spacing(1) + host(45%) + spacing(1).
                let spacing: u16 = 1;
                let num_w: u16 = 3;
                let remaining = content.width.saturating_sub(num_w + spacing + spacing);
                let host_w = (remaining as u16 * 45) / 100;
                let cell_x = match state.col {
                    TargetCol::Host => content.x + num_w + spacing,
                    TargetCol::Ports => content.x + num_w + spacing + host_w + spacing,
                };
                // Caret column within the cell: display width of the edit prefix.
                let prefix = &state.edit_buf[..state.edit_cursor.min(state.edit_buf.len())];
                let caret_offset = unicode_width::UnicodeWidthStr::width(prefix) as u16;
                let x = cell_x.saturating_add(1 + caret_offset);
                let y = content.y.saturating_add(1 /* header */ + row_in_content as u16);
                caret = Some(crate::common::editor::EditorHardwareCursor {
                    position: ratatui::layout::Position::new(x, y),
                    style: crate::common::editor::hardware_cursor_style(edtui::EditorMode::Insert),
                });
            }
        }
    }

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
        .fg(p.fg)
        .bg(p.selection_bg)
        .add_modifier(Modifier::BOLD)
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
            .fg(p.fg)
            .bg(p.selection_cell_bg)
            .add_modifier(Modifier::BOLD)
    } else if row_sel {
        Style::default().fg(p.fg).bg(p.selection_bg)
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
