//! Instance overview feature rendering.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::common::utils::text_width::truncate_from;
use crate::common::view::theme::Theme;
use dbm_store::ManagedInstance;

use super::state::OverviewState;

/// The overview rows shown for an instance, matching the original dbm: a list
/// of `(label, value)` pairs laid out as `"{label:20} {value}"`. The `value` is
/// `—` when the instance has no data for an optional field.
pub fn overview_rows(inst: &ManagedInstance, conn_count: usize) -> Vec<(String, String)> {
    let query_ready = if conn_count > 0 { "Ready" } else { "Degraded" };
    let scope = management_scope_labels(inst, conn_count);

    vec![
        ("Name".into(), inst.name.clone()),
        ("Engine".into(), inst.engine.to_string()),
        (
            "Version".into(),
            inst.version_short.clone().unwrap_or_else(|| "—".into()),
        ),
        (
            "Version (full)".into(),
            inst.version_full.clone().unwrap_or_else(|| "—".into()),
        ),
        (
            "Version checked".into(),
            inst.version_checked_at
                .clone()
                .unwrap_or_else(|| "—".into()),
        ),
        ("Instance ID".into(), inst.id.clone()),
        ("Fingerprint".into(), inst.fingerprint.clone()),
        ("Host".into(), inst.host.clone()),
        ("Port".into(), inst.port.to_string()),
        (
            "Socket".into(),
            inst.socket_path.clone().unwrap_or_else(|| "—".into()),
        ),
        (
            "Data directory".into(),
            inst.data_dir.clone().unwrap_or_else(|| "—".into()),
        ),
        (
            "Environment".into(),
            inst.env_label.clone().unwrap_or_else(|| "—".into()),
        ),
        ("Registered at".into(), inst.registered_at.clone()),
        ("Connections".into(), format!("{conn_count} registered")),
        ("Management scope".into(), scope),
        (
            "Lifecycle checked".into(),
            inst.lifecycle_checked_at
                .clone()
                .unwrap_or_else(|| "—".into()),
        ),
        ("Query readiness".into(), query_ready.into()),
    ]
}

/// The management scope labels for an instance, matching the original dbm:
/// lifecycle scope from the lifecycle status plus query/monitor/backup when the
/// instance has connections, or `—` when nothing applies.
fn management_scope_labels(inst: &ManagedInstance, conn_count: usize) -> String {
    let mut labels = Vec::new();
    match inst.lifecycle_status.as_deref() {
        Some("ready") => labels.push("lifecycle".to_string()),
        Some("degraded") => labels.push("lifecycle (degraded)".to_string()),
        _ => {}
    }
    if conn_count > 0 {
        labels.extend(["query".into(), "monitor".into(), "backup".into()]);
    }
    if labels.is_empty() {
        "—".into()
    } else {
        labels.join(", ")
    }
}

/// Render the instance overview body: the instance's attribute rows laid out
/// like the original dbm (`label:20 value`), with the cursor row highlighted.
/// Drawn inside the workspace's single outer border (the tab bar and pane
/// footer are rendered by the instance-workspace parent), so no border or
/// footer is drawn here.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &OverviewState,
    conn_count: usize,
    _focused: bool,
) {
    let p = theme.palette();

    let Some(inst) = &state.instance else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "(select an instance in the explorer)",
                Style::default().fg(p.muted),
            ))),
            area,
        );
        return;
    };

    let rows = overview_rows(inst, conn_count);
    if rows.is_empty() {
        return;
    }

    // Vertical scrolling keeps the cursor visible: the window is `viewport`
    // rows tall, anchored so the cursor stays in range.
    let viewport = area.height.max(1) as usize;
    let row_count = rows.len();
    let cursor = state.cursor.min(row_count.saturating_sub(1));
    let scroll = if cursor < viewport {
        0
    } else {
        cursor.saturating_add(1).saturating_sub(viewport)
    };

    let mut lines = Vec::new();
    for view_row in 0..viewport {
        let row_idx = scroll + view_row;
        if row_idx >= row_count {
            break;
        }
        let (label, value) = &rows[row_idx];
        let selected = row_idx == cursor;
        let style = if selected {
            Style::default()
                .fg(p.selection)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg)
        };
        let text = format!("{label:20} {value}");
        // Horizontal scroll crops the line at the pane width, like the original
        // dbm's `←/→` H-Scroll.
        let shown = truncate_from(&text, state.h_scroll as usize, area.width as usize);
        lines.push(Line::from(Span::styled(shown, style)));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbm_core::Engine;

    fn inst() -> ManagedInstance {
        ManagedInstance {
            id: "id-1".into(),
            fingerprint: "fp-1".into(),
            name: "postgres".into(),
            engine: Engine::Postgres,
            host: "127.0.0.1".into(),
            port: 5432,
            socket_path: Some("/tmp/pg.sock".into()),
            data_dir: Some("/var/lib/pg".into()),
            env_label: Some("prod".into()),
            registered_at: "2026-01-01".into(),
            version_full: Some("17.2".into()),
            version_short: Some("17".into()),
            version_checked_at: Some("2026-01-02".into()),
            lifecycle_status: Some("ready".into()),
            lifecycle_checked_at: Some("2026-01-03".into()),
            lifecycle_detail: None,
        }
    }

    #[test]
    fn overview_rows_cover_original_fields() {
        let rows = overview_rows(&inst(), 3);
        let labels: Vec<&str> = rows.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Name", "Engine", "Version", "Version (full)", "Version checked",
                "Instance ID", "Fingerprint", "Host", "Port", "Socket",
                "Data directory", "Environment", "Registered at", "Connections",
                "Management scope", "Lifecycle checked", "Query readiness",
            ]
        );
        // Connections + query readiness depend on conn_count.
        assert_eq!(rows[13].1, "3 registered");
        assert_eq!(rows[16].1, "Ready");
        // Management scope includes lifecycle + query/monitor/backup.
        assert!(rows[14].1.contains("lifecycle"));
        assert!(rows[14].1.contains("query"));
    }

    #[test]
    fn overview_rows_no_connections_is_degraded_with_dash() {
        let rows = overview_rows(&inst(), 0);
        assert_eq!(rows[13].1, "0 registered");
        assert_eq!(rows[16].1, "Degraded");
        // No connections -> management scope is only the lifecycle label.
        assert_eq!(rows[14].1, "lifecycle");
    }

    #[test]
    fn overview_rows_optional_fields_fall_back_to_dash() {
        let mut i = inst();
        i.version_short = None;
        i.socket_path = None;
        i.lifecycle_status = None;
        let rows = overview_rows(&i, 0);
        assert_eq!(rows[2].1, "—");
        assert_eq!(rows[9].1, "—");
        assert_eq!(rows[14].1, "—");
    }
}
