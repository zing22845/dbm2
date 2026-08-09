//! Instance workspace feature rendering.
//!
//! The instance workspace is a parent pane: an outer " Instance Workspace "
//! border wraps a tab bar, the active sub-pane body (overview / connections),
//! and a pane footer, all sharing that single border — matching the original
//! dbm, where the tab labels and the sub-pane content live inside one frame.
//! `focused` colors the outer border so the shell focus is visible.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::utils::text_width::wrapped_line_count;
use crate::common::view::hints::{draw_pane_footer, instance_workspace_footer_text};
use crate::common::view::theme::Theme;

use super::state::IwState;
use super::connections::view as connections_view;
use super::overview::view as overview_view;

/// The tab labels and their sub-panes, in render order. Shared by the renderer
/// and the mouse hit-test so a click targets the same regions the tabs draw in.
const IW_TABS: [(&str, crate::app_shell::nav::IwPane); 2] = [
    ("Overview", crate::app_shell::nav::IwPane::Overview),
    ("Connections", crate::app_shell::nav::IwPane::Connections),
];

/// Map a mouse hit to the instance-workspace tab under it, if any. `area` is
/// the workspace's outer (bordered) region: the tab bar is the first inner row
/// (just below the top border), starting one column after the left border.
/// Returns `None` for clicks outside any tab (e.g. the body or the border).
pub fn iw_tab_at(area: Rect, x: u16, y: u16) -> Option<crate::app_shell::nav::IwPane> {
    if y != area.y + 1 {
        return None;
    }
    let mut cursor_x = area.x + 1;
    for (label, pane) in IW_TABS {
        // `[ {label} ]` = label width + 4; tabs are separated by two spaces.
        let w = label.len() as u16 + 4;
        if x >= cursor_x && x < cursor_x + w {
            return Some(pane);
        }
        cursor_x += w + 2;
    }
    None
}

/// Render the instance workspace parent pane: an outer " Instance Workspace ·
/// <instance> " border (title carries the open instance name) wraps the tab
/// bar, the active sub-pane body and a pane footer, all sharing that single
/// border (matching the original dbm). `focused` colors the outer border.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &IwState,
    focused: bool,
) {
    let p = theme.palette();
    // Title shows the open instance's name next to the workspace label:
    // " Instance Workspace · <instance> ". Falls back to the bare label when no
    // instance is open yet.
    let title = if state.instance_name.is_empty() {
        " Instance Workspace ".to_string()
    } else {
        format!(" Instance Workspace · {} ", state.instance_name)
    };
    let outer = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    if inner.width == 0 || inner.height < 3 {
        return;
    }

    // The pane footer wraps its hint line, so reserve a row for it and size the
    // pane footer to the wrapped height (a narrow pane can wrap the keys).
    let footer_text = instance_workspace_footer_text(state.pane);
    let footer_h = wrapped_line_count(&footer_text, inner.width)
        .max(1)
        .min(inner.height.saturating_sub(2).max(1));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tab bar
            Constraint::Min(0),    // active pane body
            Constraint::Length(footer_h),
        ])
        .split(inner);

    // Tab bar listing the child sub-panes; the active one is highlighted.
    let mut spans = Vec::new();
    for (idx, (label, pane)) in IW_TABS.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw("  "));
        }
        let selected = *pane == state.pane;
        let text = format!("[ {} ]", label);
        let style = if selected {
            Style::default()
                .fg(p.fg)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default().fg(p.muted)
        };
        spans.push(Span::styled(text, style));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[0]);

    // Body shows only the active sub-pane (like a tab page), rendered without
    // its own border — the outer workspace border frames it (matching the
    // original dbm). The pane footer is drawn by this parent on the bottom row.
    match state.pane {
        crate::app_shell::nav::IwPane::Overview => {
            // The overview shows "Connections: N registered" and query
            // readiness, so it needs the connection count from the sibling pane.
            let conn_count = state.connections.connections.len();
            overview_view::render(frame, theme, chunks[1], &state.overview, conn_count, focused)
        }
        crate::app_shell::nav::IwPane::Connections => {
            connections_view::render(frame, theme, chunks[1], &state.connections, focused)
        }
    }
    draw_pane_footer(frame, theme, chunks[2], &footer_text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_shell::nav::IwPane;

    #[test]
    fn iw_tab_at_hits_tabs_and_ignores_other_rows() {
        // Workspace outer region at (40, 3), tab bar on the first inner row.
        let area = Rect::new(40, 3, 60, 20);
        let tab_y = area.y + 1;
        // Overview tab spans `[ Overview ]` (12 cols) starting at x=41.
        assert_eq!(iw_tab_at(area, 41, tab_y), Some(IwPane::Overview));
        assert_eq!(iw_tab_at(area, 52, tab_y), Some(IwPane::Overview));
        // Two-space gap after Overview is a miss.
        assert_eq!(iw_tab_at(area, 53, tab_y), None);
        // Connections tab starts at 41 + 12 + 2 = 55 and spans 15 cols.
        assert_eq!(iw_tab_at(area, 55, tab_y), Some(IwPane::Connections));
        assert_eq!(iw_tab_at(area, 69, tab_y), Some(IwPane::Connections));
        // Clicks outside the tab bar (e.g. the body row) hit no tab.
        assert_eq!(iw_tab_at(area, 41, area.y + 2), None);
        assert_eq!(iw_tab_at(area, 41, area.y), None);
    }
}