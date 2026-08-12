//! SQL tab bar: per-connection tab titles, themed rendering and click rects.
//!
//! Each tab's title uses the original dbm `<SQL {sequence}>` format where
//! `sequence` is a per-connection counter starting at 1. Only tabs belonging
//! to the active connection are rendered. Clickable rects are returned so the
//! shell can route mouse clicks to a tab.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::session::TabSession;

/// A rendered tab: its clickable rect and the visible-tab offset (0-based
/// within the active connection's tabs). This offset maps directly to
/// `SqlTabMessage::Tab(idx)` / `SqlTabMessage::CloseTab(idx)`.
#[derive(Debug, Clone)]
pub struct TabRect {
    pub rect: Rect,
    pub tab_index: usize,
}

/// Derive a short tab title from the per-connection sequence, formatted in the
/// original dbm `<SQL {sequence}>` style.
pub fn tab_title(session: &TabSession) -> String {
    format!("<SQL {}>", session.sequence)
}

/// Render the tab bar into `area`, showing only the tabs whose global indices
/// are in `visible_indices`. Returns clickable rects for each rendered tab.
///
/// `active_global` is the global index of the currently active tab, used to
/// highlight it in the tab bar.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    tabs: &[TabSession],
    visible_indices: &[usize],
    active_global: Option<usize>,
) -> Vec<TabRect> {
    let p = theme.palette();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut rects: Vec<TabRect> = Vec::new();
    let mut x = area.x;

    for (visible_idx, &global_idx) in visible_indices.iter().enumerate() {
        let Some(session) = tabs.get(global_idx) else {
            continue;
        };
        let active = active_global == Some(global_idx);
        let label = format!(" {} ", tab_title(session));
        let width = label.chars().count() as u16;
        rects.push(TabRect {
            rect: Rect {
                x,
                y: area.y,
                width,
                height: area.height,
            },
            tab_index: visible_idx,
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

    fn session_with_sequence(seq: usize) -> TabSession {
        TabSession {
            sequence: seq,
            ..TabSession::default()
        }
    }

    #[test]
    fn title_uses_per_connection_sequence() {
        assert_eq!(tab_title(&session_with_sequence(1)), "<SQL 1>");
        assert_eq!(tab_title(&session_with_sequence(5)), "<SQL 5>");
        assert_eq!(tab_title(&session_with_sequence(42)), "<SQL 42>");
    }
}
