//! Discovery targets editor feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::{TargetCol, TargetsState};

/// Render the targets editor: a bordered list of `host : ports` rows, with the
/// focused cell highlighted and an inline edit shown when editing.
pub fn render(frame: &mut Frame, theme: &Theme, area: Rect, state: &TargetsState) {
    let p = theme.palette();

    let mut lines = Vec::new();
    let inner_h = area.height.saturating_sub(2) as usize;
    for (vis, idx) in (state.scroll..state.targets.len()).enumerate() {
        if vis >= inner_h {
            break;
        }
        lines.push(row_line(theme, &state.targets[idx], idx == state.row, state.col, state.editing, &state.edit_buf));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled("(no targets)", Style::default().fg(p.muted))));
    }

    let block = Block::default()
        .title(" targets ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border_active));
    frame.render_widget(Paragraph::new(lines).block(block), area);
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
