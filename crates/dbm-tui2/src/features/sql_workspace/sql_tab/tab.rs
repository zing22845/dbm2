//! SQL tab bar: session-derived titles, themed rendering and click rects.
//!
//! Each tab's title is derived from its session identity (connection / database
//! name) with a stable `SQL {id}` fallback. Clickable rects are returned so the
//! shell can route mouse clicks to a tab.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::session::TabSession;

/// A rendered tab: its clickable rect and the underlying tab index.
#[derive(Debug, Clone)]
pub struct TabRect {
    pub rect: Rect,
    pub tab_index: usize,
}

/// Derive a short title for a tab from its session identity.
pub fn tab_title(session: &TabSession, tab_id: usize) -> String {
    match (&session.connection, &session.database) {
        (Some(conn), Some(db)) => format!("{conn}/{db}"),
        (Some(conn), None) => conn.clone(),
        (None, Some(db)) => db.clone(),
        (None, None) => format!("SQL {tab_id}"),
    }
}

/// Render the tab bar into `area`, returning clickable rects for each tab.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    tabs: &[TabSession],
    active_tab: Option<usize>,
) -> Vec<TabRect> {
    let p = theme.palette();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut rects: Vec<TabRect> = Vec::new();
    let mut x = area.x;

    for (global_idx, session) in tabs.iter().enumerate() {
        let active = active_tab == Some(global_idx);
        let label = format!(" {} ", tab_title(session, global_idx));
        let width = label.chars().count() as u16;
        rects.push(TabRect {
            rect: Rect {
                x,
                y: area.y,
                width,
                height: area.height,
            },
            tab_index: global_idx,
        });
        x = x.saturating_add(width);
        let style = if active {
            Style::default()
                .fg(p.surface)
                .bg(p.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg)
        };
        spans.push(Span::styled(label, style));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(instance: &str, conn: &str, db: Option<&str>) -> TabSession {
        TabSession {
            id: 0,
            connection_id: Some("c1".into()),
            instance: Some(instance.into()),
            connection: Some(conn.into()),
            database: db.map(Into::into),
            schema: None,
        }
    }

    #[test]
    fn title_combines_connection_and_database() {
        let s = session("local", "app-db", Some("mydb"));
        assert_eq!(tab_title(&s, 0), "app-db/mydb");
    }

    #[test]
    fn title_falls_back_to_connection_then_sql_id() {
        assert_eq!(tab_title(&session("local", "app-db", None), 2), "app-db");
        assert_eq!(tab_title(&TabSession::default(), 7), "SQL 7");
    }
}
