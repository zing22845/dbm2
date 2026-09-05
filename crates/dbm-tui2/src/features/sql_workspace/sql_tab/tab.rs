//! SQL tab bar: per-connection tab titles, themed rendering and click rects.
//!
//! Each tab's title uses the original dbm `<SQL {sequence}>` format where
//! `sequence` is a per-connection counter starting at 1. Only tabs belonging
//! to the active connection are rendered. Clickable rects are returned so the
//! shell can route mouse clicks to a tab.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

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

/// Compute the clickable rect for each rendered tab, laid out left-to-right in
/// the tab bar. The geometry is shared by rendering and mouse hit-testing so a
/// clickable tab is always where it is drawn.
pub fn tab_rects(area: Rect, tabs: &[TabSession], visible_indices: &[usize]) -> Vec<TabRect> {
    let mut rects: Vec<TabRect> = Vec::new();
    let mut x = area.x;
    for (visible_idx, &global_idx) in visible_indices.iter().enumerate() {
        let Some(session) = tabs.get(global_idx) else {
            continue;
        };
        let width = tab_title(session).chars().count() as u16 + 2; // " title "
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
    }
    rects
}

/// Hit-test a click at `(x, y)` against the rendered tab bar. Returns the
/// visible-tab offset of the clicked tab, if any.
pub fn tab_at(
    area: Rect,
    tabs: &[TabSession],
    visible_indices: &[usize],
    x: u16,
    y: u16,
) -> Option<usize> {
    if y != area.y {
        return None;
    }
    tab_rects(area, tabs, visible_indices)
        .into_iter()
        .find(|t| x >= t.rect.x && x < t.rect.x.saturating_add(t.rect.width))
        .map(|t| t.tab_index)
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
    let rects = tab_rects(area, tabs, visible_indices);
    let mut spans: Vec<Span<'static>> = Vec::new();
    for &global_idx in visible_indices.iter() {
        let active = active_global == Some(global_idx);
        let label = tab_title(tabs.get(global_idx).expect("visible index valid"));
        let style = if active {
            Style::default()
                .fg(p.surface)
                .bg(p.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg)
        };
        spans.push(Span::styled(format!(" {label} "), style));
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

    #[test]
    fn tab_at_hit_tests_by_position() {
        let sessions = [session_with_sequence(1), session_with_sequence(2)];
        let visible = [0usize, 1];
        // area starting at x=10: "<SQL 1> " then "<SQL 2> " laid left-to-right.
        let area = Rect::new(10, 5, 40, 1);
        let w1 = tab_title(&sessions[0]).chars().count() as u16 + 2;
        // Click on the first tab title.
        assert_eq!(tab_at(area, &sessions, &visible, 10, 5), Some(0));
        // Click on the second tab title (just past the first tab's width).
        assert_eq!(tab_at(area, &sessions, &visible, 10 + w1 + 1, 5), Some(1));
        // Click outside the tab bar row (y != area.y) -> none.
        assert_eq!(tab_at(area, &sessions, &visible, 10, 6), None);
        // Click in empty space to the right of both tabs -> none.
        assert_eq!(tab_at(area, &sessions, &visible, 200, 5), None);
    }
}
