//! Instance connections feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::{ConnectionsState, FormField};

/// Render the connections panel: the connection list plus (when open) the
/// add/edit form. The list body is drawn inside the workspace's single outer
/// border (the tab bar and pane footer are rendered by the parent), so no
/// border or footer is drawn here — matching the original dbm.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ConnectionsState,
    _region_focused: bool,
) {
    let p = theme.palette();

    if let Some(form) = &state.form {
        render_form(frame, theme, area, state, form);
        return;
    }

    let mut lines = Vec::new();
    for (idx, conn) in state.connections.iter().enumerate() {
        let focused = idx == state.cursor;
        let style = if focused {
            Style::default()
                .fg(p.selection)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg)
        };
        let pw = if conn.has_password { " (pw)" } else { "" };
        lines.push(Line::from(vec![
            Span::styled(format!("  {}@.../{}", conn.username, conn.database), style),
            Span::styled(format!("  {} {pw}", conn.name), Style::default().fg(p.fg_dim)),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no connections — a to add)",
            Style::default().fg(p.muted),
        )));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_form(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    _state: &ConnectionsState,
    form: &super::state::ConnectionForm,
) {
    let p = theme.palette();
    let field_style = |f: FormField| {
        if form.field == f {
            Style::default().fg(p.selection).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg)
        }
    };
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Name: ", field_style(FormField::Name)),
        Span::raw(format!("  {}", form.name)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("User: ", field_style(FormField::Username)),
        Span::raw(format!("  {}", form.username)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("DB:   ", field_style(FormField::Database)),
        Span::raw(format!("  {}", form.database)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Pass: ", field_style(FormField::Password)),
        Span::raw("  ***"),
    ]));
    let block = Block::default()
        .title(" connection ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border_active));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
