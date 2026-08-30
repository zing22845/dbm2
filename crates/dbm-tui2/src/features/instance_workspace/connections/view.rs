//! Instance connections feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Row, Table};
use ratatui::Frame;

use crate::common::view::pane_scrollbar::{
    PaneScrollLayout, draw_vertical_pane_scrollbar, pane_scroll_layout,
};
use crate::common::view::theme::Theme;

use super::state::{ConnectionsState, FormField};

/// Number of header rows in the connections Table widget.
const HEADER_H: u16 = 1;

/// Result of [`compute_connections_viewport`]: shared by render and
/// v_scrollbar_hit so they agree on geometry and scroll position.
pub struct ConnectionsViewport {
    /// Header row rect (fixed, does not scroll).
    pub header: Rect,
    /// Original data_body rect (before pane_scroll_layout reserves v_bar).
    pub data_body: Rect,
    /// Layout returned by pane_scroll_layout on data_body.
    pub layout: PaneScrollLayout,
    /// The actual Table render area (inside data_body, reserved v_bar).
    pub content: Rect,
    /// Number of data rows visible in content.
    pub viewport: usize,
    /// Scroll offset: index of the first visible data row.
    pub start: usize,
    /// Total data rows.
    pub total: usize,
    /// Max possible scroll offset (= total - viewport, clamped to >= 0).
    pub max_scroll: usize,
}

/// Shared viewport computation. Applies discover-style cursor anchoring
/// (skipped when `scroll_locked`).
pub fn compute_connections_viewport(
    area: Rect,
    state: &ConnectionsState,
) -> Option<ConnectionsViewport> {
    let total = state.connections.len();
    if total == 0 {
        return None;
    }

    // Split off the fixed header row; the rest is the scrollable data body.
    if area.height <= HEADER_H {
        return None;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(HEADER_H),
            Constraint::Min(0),
        ])
        .split(area);
    let header = chunks[0];
    let data_body = chunks[1];
    if data_body.height == 0 {
        return None;
    }

    // Horizontal content width estimate: connections columns are Name(16) +
    // Target(min 12) + SSL(8) + Password(10) + Updated(19) + Test OK(19) +
    // Test Fail(19) = 103 fixed (Target min can grow). Connections does not
    // wrap or h_scroll in practice, so use 0 to skip h_scrollbar reservation.
    let layout = pane_scroll_layout(data_body, 0, total, data_body.height as usize);
    let content = layout.content_area;

    let viewport = content.height.max(1) as usize;
    let max_scroll = total.saturating_sub(viewport);

    // Discover-style anchor via shared helper.
    let start = crate::common::view::pane_scrollbar::discover_anchor(
        state.scroll,
        max_scroll,
        state.cursor,
        viewport,
        state.scroll_locked,
    );

    Some(ConnectionsViewport {
        header,
        data_body,
        layout,
        content,
        viewport,
        start,
        total,
        max_scroll,
    })
}

use crate::common::view::pane_scrollbar::ScrollbarHitInfo;

/// Hit-test the connections pane's vertical scrollbar — delegates to shared helper.
pub fn v_scrollbar_hit(
    area: Rect,
    state: &ConnectionsState,
    x: u16,
    y: u16,
) -> Option<ScrollbarHitInfo> {
    let cv = compute_connections_viewport(area, state)?;
    crate::common::view::pane_scrollbar::v_scrollbar_hit(&cv.layout, cv.max_scroll, x, y)
}

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
    // Always render the connection list first so it stays visible behind the
    // add/edit popup (the popup only covers its own rect).
    if state.connections.is_empty() {
        frame.render_widget(
            ratatui::widgets::Paragraph::new("No connections yet — press a to add"),
            area,
        );
        return;
    }

    let cv = match compute_connections_viewport(area, state) {
        Some(v) => v,
        None => {
            // Area too small — header row alone is fine; Table below would
            // produce no rows so skip entirely.
            return;
        }
    };

    render_connections_list(frame, theme, &cv, state, instance);

    // The add/edit form is drawn last as a centered popup on top of the list.
    if let Some(form) = &state.form {
        render_form(frame, theme, area, state, form);
    }
}

fn render_connections_list(
    frame: &mut Frame,
    theme: &Theme,
    cv: &ConnectionsViewport,
    state: &ConnectionsState,
    instance: Option<&dbm_store::ManagedInstance>,
) {
    let p = theme.palette();

    // ---- HEADER (fixed row, never scrolls) ----
    let header_row = Row::new(vec![
        Cell::from("Name"),
        Cell::from("Target"),
        Cell::from("SSL"),
        Cell::from("Password"),
        Cell::from("Updated"),
        Cell::from("Test OK"),
        Cell::from("Test Fail"),
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));

    // ---- DATA ROWS (sliced to the visible viewport) ----
    let end = (cv.start + cv.viewport).min(cv.total);
    let rows: Vec<Row> = (cv.start..end)
        .map(|i| {
            let conn = &state.connections[i];
            let selected = i == state.cursor;
            let color = match (
                conn.test_succeeded_at.as_deref(),
                conn.test_failed_at.as_deref(),
            ) {
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
                let fg = if color == p.fg { p.selection_text } else { color };
                Style::default()
                    .fg(fg)
                    .bg(p.selection_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(color)
            };
            let name_body = conn.name.clone();
            let password = if conn.has_password { "set" } else { "empty" };
            let target = instance
                .map_or_else(|| "?".to_string(), |inst| conn.display_target(inst));
            let ok_at = conn
                .test_succeeded_at
                .clone()
                .unwrap_or_else(|| "—".into());
            let fail_at = conn
                .test_failed_at
                .clone()
                .unwrap_or_else(|| "—".into());
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

    use ratatui::layout::Constraint;
    let constraints = [
        Constraint::Length(16),
        Constraint::Min(12),
        Constraint::Length(8),
        Constraint::Length(10),
        Constraint::Length(19),
        Constraint::Length(19),
        Constraint::Length(19),
    ];

    // Render header on its own row and Table body in the content area.
    let table = Table::new(rows, constraints).header(header_row);
    frame.render_widget(table, cv.data_body);

    // ---- VERTICAL SCROLLBAR ----
    if let Some(bar) = cv.layout.v_scrollbar {
        draw_vertical_pane_scrollbar(
            frame,
            bar,
            cv.start,
            cv.viewport,
            cv.max_scroll,
            p,
            false,
        );
    }
}

// ---------------------------------------------------------------------------
//  Form rendering (unchanged from original)
// ---------------------------------------------------------------------------

fn render_form(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ConnectionsState,
    form: &super::state::ConnectionForm,
) {
    let p = theme.palette();
    use super::state::FormMode;
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
    let field_value = |f: FormField, value: &str| -> String {
        if form.field == f && form.mode == FormMode::Insert {
            format!("{value}█")
        } else {
            value.to_string()
        }
    };
    let password_display = if form.password.is_empty() {
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
    let footer_text = match form.mode {
        FormMode::Insert => "Keep: ENTER  Revert: ESC".to_string(),
        FormMode::Normal => {
            "Edit: i  Clear+Edit: dd  Test: t  Save: ENTER  Cancel: ESC".to_string()
        }
    };

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

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_conn(name: &str) -> dbm_store::InstanceConnection {
        dbm_store::InstanceConnection {
            id: name.into(),
            instance_id: "inst".into(),
            name: name.into(),
            username: "postgres".into(),
            database: "postgres".into(),
            has_password: false,
            ssl_mode: "prefer".into(),
            env_label: None,
            created_at: "now".into(),
            updated_at: "now".into(),
            test_succeeded_at: None,
            test_failed_at: None,
        }
    }

    #[test]
    fn viewport_does_not_scroll_when_fewer_rows_than_viewport() {
        let mut s = ConnectionsState::default();
        s.connections = (0..3).map(|i| mk_conn(&format!("c{i}"))).collect();
        // Small area but still big enough to show all 3 data rows + 1 header.
        let area = Rect::new(0, 0, 80, 10);
        let cv = compute_connections_viewport(area, &s).expect("some viewport");
        assert_eq!(cv.start, 0);
        assert_eq!(cv.max_scroll, 0);
    }

    #[test]
    fn viewport_anchors_cursor_to_viewport_when_scroll_not_locked() {
        let mut s = ConnectionsState::default();
        s.connections = (0..20).map(|i| mk_conn(&format!("c{i}"))).collect();
        let area = Rect::new(0, 0, 80, 6); // header(1) + 5 data rows visible
        // cursor at row 15, which is beyond the 5-row viewport (viewport rows
        // top out at 4 when start=0).
        s.cursor = 15;
        s.scroll = 0;
        s.scroll_locked = false;
        let cv = compute_connections_viewport(area, &s).expect("some viewport");
        assert_eq!(cv.start, 11, "anchor: cursor=15 with viewport=5 -> start=15-5+1");
    }

    #[test]
    fn viewport_respects_scroll_locked() {
        let mut s = ConnectionsState::default();
        s.connections = (0..20).map(|i| mk_conn(&format!("c{i}"))).collect();
        let area = Rect::new(0, 0, 80, 6);
        s.cursor = 0;
        s.scroll = 10; // manual drag scrolled to row 10
        s.scroll_locked = true;
        let cv = compute_connections_viewport(area, &s).expect("some viewport");
        assert_eq!(cv.start, 10, "scroll_locked keeps manual scroll even if cursor is elsewhere");
    }

    #[test]
    fn v_scrollbar_hit_returns_none_when_no_scroll_needed() {
        let mut s = ConnectionsState::default();
        s.connections = vec![mk_conn("only")];
        let area = Rect::new(0, 0, 80, 10);
        // Even clicking the far right column should not produce a hit.
        let hit = v_scrollbar_hit(area, &s, 79, 5);
        assert!(hit.is_none());
    }
}
