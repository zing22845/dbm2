//! Discovery targets editor feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::{TargetCol, TargetsState};

/// Render the targets editor: a bordered list of `host : ports` rows, with the
/// focused cell highlighted and an inline edit shown when editing. The border
/// highlights only when the targets pane owns focus, and a footer line shows
/// the active target keys.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &TargetsState,
    focus: crate::app_shell::pane::DiscoverPane,
) {
    use crate::common::view::hints::{discover_targets_footer_text, draw_pane_footer};
    let p = theme.palette();
    let focused = focus == crate::app_shell::pane::DiscoverPane::Targets;

    let footer_text = discover_targets_footer_text(state.editing, state.has_loopback());
    let footer_h = if footer_text.is_empty() {
        0
    } else {
        footer_text.lines().count().clamp(1, 2) as u16
    };

    let block = Block::default()
        .title(" targets ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { p.border_active } else { p.border }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // Body is the inner area minus the footer strip. Rendered as a table with
    // `#` line-number, `Host` and `Ports` columns (matching the original dbm),
    // so host and ports are independently editable and visible.
    let body = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer_h),
    );
    if body.width > 0 && body.height > 0 {
        let selected = focused && state.row < state.targets.len();
        let header = ratatui::widgets::Row::new(["#", "Host", "Ports"])
            .style(Style::default().add_modifier(Modifier::BOLD));
        let rows = state
            .targets
            .iter()
            .enumerate()
            .skip(state.scroll)
            .take(body.height as usize)
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
        frame.render_widget(table, body);
    }

    if footer_h > 0 {
        let footer_area = Rect::new(inner.x, inner.y + body.height, inner.width, footer_h);
        draw_pane_footer(frame, theme, footer_area, &footer_text);
    }
}

/// Style for the focused/selected target row (a subtle selection background).
fn row_style(theme: &Theme) -> Style {
    let p = theme.palette();
    Style::default()
        .fg(p.fg)
        .bg(p.surface)
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
        row_style(theme)
    } else if row_sel {
        Style::default().fg(p.selection)
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
