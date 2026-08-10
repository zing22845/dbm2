//! Instance connections feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::{ConnectionsState, FormField};

/// Render the connections panel: the connection list plus (when open) the
/// add/edit form. The list body is drawn inside the workspace's single outer
/// border (the tab bar and pane footer are rendered by the parent), so no
/// border or footer is drawn here — matching the original dbm. `instance` is the
/// managed instance's host/port for the Target column.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ConnectionsState,
    instance: Option<&dbm_store::ManagedInstance>,
    _region_focused: bool,
) {
    let p = theme.palette();

    // Always render the connection list first so it stays visible behind the
    // add/edit popup (the popup only covers its own rect).
    if state.connections.is_empty() {
        frame.render_widget(
            ratatui::widgets::Paragraph::new("No connections yet — press a to add"),
            area,
        );
    } else {
        use ratatui::layout::Constraint;
        use ratatui::widgets::{Cell, Row, Table};
        const COL_NAME: u16 = 16;
        let constraints = [
            Constraint::Length(COL_NAME),
            Constraint::Min(12),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(19),
            Constraint::Length(19), // Test Succeeded
            Constraint::Length(19), // Test Failed
        ];
        let header = Row::new(vec![
            Cell::from("Name"),
            Cell::from("Target"),
            Cell::from("SSL"),
            Cell::from("Password"),
            Cell::from("Updated"),
            Cell::from("Test OK"),
            Cell::from("Test Fail"),
        ])
        .style(Style::default().add_modifier(Modifier::BOLD));

        let rows: Vec<Row> = state
            .connections
            .iter()
            .enumerate()
            .map(|(i, conn)| {
                let selected = i == state.cursor;
                // The whole row is green when the most recent test succeeded and
                // red when it failed (the later timestamp wins); rows never
                // tested use the normal fg.
                let color = match (conn.test_succeeded_at.as_deref(), conn.test_failed_at.as_deref())
                {
                    (Some(s), Some(f)) => {
                        if s > f {
                            ratatui::style::Color::Green
                        } else {
                            ratatui::style::Color::Red
                        }
                    }
                    (Some(_), None) => ratatui::style::Color::Green,
                    (None, Some(_)) => ratatui::style::Color::Red,
                    (None, None) => p.fg,
                };
                let style = if selected {
                    Style::default()
                        .fg(color)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(color)
                };
                let name_body = conn.name.clone();
                let password = if conn.has_password { "set" } else { "empty" };
                let target =
                    instance.map_or_else(|| "?".to_string(), |inst| conn.display_target(inst));
                let ok_at = conn.test_succeeded_at.clone().unwrap_or_else(|| "—".into());
                let fail_at = conn.test_failed_at.clone().unwrap_or_else(|| "—".into());
                Row::new(vec![
                    Cell::from(name_body),
                    Cell::from(target),
                    Cell::from(conn.ssl_mode.clone()),
                    Cell::from(password),
                    Cell::from(conn.updated_at.clone()),
                    Cell::from(ok_at),
                    Cell::from(fail_at),
                ])
                .style(style)
            })
            .collect();

        let table = Table::new(rows, constraints).header(header);
        frame.render_widget(table, area);
    }

    // The add/edit form is drawn last as a centered popup on top of the list.
    if let Some(form) = &state.form {
        render_form(frame, theme, area, state, form);
    }
}

fn render_form(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ConnectionsState,
    form: &super::state::ConnectionForm,
) {
    let p = theme.palette();
    use super::state::FormMode;
    // Title matches the original dbm: "Add/Edit connection on `<instance>`",
    // with the current input mode (INSERT/NORMAL) appended.
    let instance = if state.instance_name.is_empty() {
        "?".to_string()
    } else {
        state.instance_name.clone()
    };
    let verb = if form.edit_original_name.is_some() {
        "Edit"
    } else {
        "Add"
    };
    let mode = match form.mode {
        FormMode::Insert => "INSERT",
        FormMode::Normal => "NORMAL",
    };
    let title = format!(" {verb} connection on `{instance}` [{mode}] ");

    let field_style = |f: FormField| {
        if form.field == f {
            Style::default().fg(p.selection).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg)
        }
    };
    // In insert mode the active field shows a block cursor at the end.
    let field_value = |f: FormField, value: &str| -> String {
        if form.field == f && form.mode == FormMode::Insert {
            format!("{value}█")
        } else {
            value.to_string()
        }
    };
    let password_display = if form.password.is_empty() {
        // Editing with no typed password keeps the stored one (not shown).
        if form.edit_original_name.is_some() {
            "(unchanged)".to_string()
        } else {
            "".to_string()
        }
    } else {
        "****".to_string()
    };
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("name", field_style(FormField::Name)),
        Span::raw(format!(":  {}", field_value(FormField::Name, &form.name))),
    ]));
    lines.push(Line::from(vec![
        Span::styled("user", field_style(FormField::Username)),
        Span::raw(format!(":  {}", field_value(FormField::Username, &form.username))),
    ]));
    lines.push(Line::from(vec![
        Span::styled("database", field_style(FormField::Database)),
        Span::raw(format!(":  {}", field_value(FormField::Database, &form.database))),
    ]));
    lines.push(Line::from(vec![
        Span::styled("password", field_style(FormField::Password)),
        Span::raw(format!(":  {}", field_value(FormField::Password, &password_display))),
    ]));
    // The form's operation hints (matching the original dbm) live on a footer
    // row inside the popup, under the fields.
    let footer_text = match form.mode {
        FormMode::Insert => "Keep: ENTER  Revert: ESC".to_string(),
        FormMode::Normal => {
            "Edit: i  Clear+Edit: dd  Test: t  Save: ENTER  Cancel: ESC".to_string()
        }
    };

    // Render the form as a centered modal popup over the connections pane — the
    // same popup family as the discover modal. Only the popup's own rectangle is
    // covered, so the surrounding connection list stays visible behind it.
    crate::common::view::modal::render_modal_popup(
        frame,
        theme,
        area,
        62,
        58,
        state,
        |f, t, popup, s| {
            use ratatui::layout::{Constraint, Layout};
            use ratatui::widgets::{Block, Borders, Paragraph};
            use crate::common::utils::text_width::wrapped_line_count;
            use super::state::ConnectionStatusKind;
            let pp = t.palette();
            let block = Block::default()
                .title(title.clone())
                .borders(Borders::ALL)
                .border_style(Style::default().fg(pp.border_active));
            let inner = block.inner(popup);
            f.render_widget(block, popup);
            // Footer has an operation-hint row plus (when present) the test
            // result row. Both rows can wrap on a narrow popup, so size the
            // footer to the wrapped line count.
            let hint_h = wrapped_line_count(&footer_text, inner.width.max(1))
                .max(1)
                .min(inner.height.saturating_sub(2).max(1));
            let status_h = s.status.as_deref().map_or(0, |st| {
                wrapped_line_count(st, inner.width.max(1)).max(1)
            });
            let mut constraints = vec![Constraint::Min(1), Constraint::Length(hint_h)];
            if status_h > 0 {
                constraints.push(Constraint::Length(status_h));
            }
            let areas = Layout::vertical(constraints).split(inner);
            f.render_widget(Paragraph::new(lines.clone()), areas[0]);
            f.render_widget(
                Paragraph::new(footer_text.clone()).style(Style::default().fg(pp.muted)),
                areas[1],
            );
            if let Some(status) = s.status.as_deref() {
                // The test result is colored like the original dbm: green on
                // success, red on failure.
                let color = match s.status_kind {
                    ConnectionStatusKind::Success => ratatui::style::Color::Green,
                    ConnectionStatusKind::Failure => ratatui::style::Color::Red,
                    ConnectionStatusKind::Idle => pp.muted,
                };
                f.render_widget(
                    Paragraph::new(status)
                        .style(Style::default().fg(color))
                        .wrap(ratatui::widgets::Wrap { trim: false }),
                    areas[2],
                );
            }
        },
    );
}
