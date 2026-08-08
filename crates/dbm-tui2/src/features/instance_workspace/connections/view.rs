//! Instance connections feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::hints::{draw_pane_footer, instance_workspace_footer_text};
use crate::common::view::theme::Theme;
use crate::app_shell::nav::IwPane;

use super::state::{ConnectionsState, FormField};

/// Render the connections panel: the connection list plus (when open) the
/// add/edit form. `region_focused` controls the border color.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ConnectionsState,
    region_focused: bool,
) {
    let p = theme.palette();

    if let Some(form) = &state.form {
        render_form(frame, theme, area, state, form);
        return;
    }

    // The block is drawn over `area`; its inner area is split into a body (the
    // connection list) and a footer hint line at the bottom, both *inside* the
    // pane's border — matching the original dbm.
    let block = Block::default()
        .title(" connections ")
        .borders(Borders::ALL)
        .border_style(p.active_border(region_focused));
    frame.render_widget(&block, area);
    let inner = block.inner(area);
    let (body, footer_area) = if inner.height > 1 {
        let h = inner.height.saturating_sub(1);
        (
            Rect {
                x: inner.x,
                y: inner.y,
                width: inner.width,
                height: h,
            },
            Rect {
                x: inner.x,
                y: inner.y.saturating_add(h),
                width: inner.width,
                height: 1,
            },
        )
    } else {
        (inner, Rect::default())
    };

    let mut lines = Vec::new();
    let inner_h = body.height as usize;
    for (vis, idx) in (0..state.connections.len()).enumerate() {
        if vis >= inner_h {
            break;
        }
        let conn = &state.connections[idx];
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

    frame.render_widget(Paragraph::new(lines), body);
    // Pane footer (inside the border): Add/Edit/Delete/Test.
    draw_pane_footer(
        frame,
        theme,
        footer_area,
        &instance_workspace_footer_text(IwPane::Connections),
    );
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
