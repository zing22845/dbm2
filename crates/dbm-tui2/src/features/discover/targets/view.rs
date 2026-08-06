//! Discovery targets editor feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
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
    focus: crate::features::discover::state::DiscoverFocus,
) {
    use crate::common::view::hints::{discover_targets_footer_text, draw_pane_footer};
    let p = theme.palette();
    let focused = focus == crate::features::discover::state::DiscoverFocus::Targets;

    let footer_text = discover_targets_footer_text(state.editing);
    let footer_h = if footer_text.is_empty() { 0 } else { 1 };

    let block = Block::default()
        .title(" targets ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { p.border_active } else { p.border }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // Body is the inner area minus the footer strip.
    let body = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer_h),
    );
    let mut lines = Vec::new();
    let body_h = body.height as usize;
    for (vis, idx) in (state.scroll..state.targets.len()).enumerate() {
        if vis >= body_h {
            break;
        }
        lines.push(row_line(theme, &state.targets[idx], idx == state.row, state.col, state.editing, &state.edit_buf));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled("(no targets)", Style::default().fg(p.muted))));
    }
    frame.render_widget(Paragraph::new(lines), body);

    if footer_h > 0 {
        let footer_area = Rect::new(inner.x, inner.y + body.height, inner.width, footer_h);
        draw_pane_footer(frame, theme, footer_area, &footer_text);
    }
}

/// Build a single target row line, styling the focused cell and rendering the
/// in-progress edit in place.
fn row_line(
    theme: &Theme,
    row: &super::state::TargetRow,
    row_focused: bool,
    col: TargetCol,
    editing: bool,
    edit_buf: &str,
) -> Line<'static> {
    let p = theme.palette();
    let sel = if row_focused {
        Style::default()
            .fg(p.selection)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(p.fg)
    };

    let host_display = if editing && col == TargetCol::Host && row_focused {
        edit_buf.to_string()
    } else {
        row.host.clone()
    };
    let ports_display = if editing && col == TargetCol::Ports && row_focused {
        edit_buf.to_string()
    } else {
        row.ports_spec.clone()
    };

    Line::from(vec![
        Span::styled(" ", sel),
        Span::styled(format!(" {host_display} "), sel),
        Span::styled(":", Style::default().fg(p.muted)),
        Span::styled(format!(" {ports_display}"), sel),
    ])
}
