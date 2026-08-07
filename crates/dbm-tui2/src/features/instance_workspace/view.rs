//! Instance workspace feature rendering.
//!
//! The instance workspace is a parent pane: an outer " Instance Workspace "
//! border wraps a tab bar plus the body of the active sub-pane (overview /
//! connections), mirroring the original dbm's instance pane tab bar. `focused`
//! colors the outer border so the shell focus is visible.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::IwState;
use super::connections::view as connections_view;
use super::overview::view as overview_view;

/// Render the instance workspace parent pane: an outer " Instance Workspace "
/// border, a tab bar for the child sub-panes (overview / connections), and the
/// selected sub-pane's body below. `focused` colors the outer border.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &IwState,
    focused: bool,
) {
    let p = theme.palette();
    let border_color = if focused { p.border_active } else { p.border };
    let outer = Block::default()
        .title(" Instance Workspace ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    if inner.width == 0 || inner.height < 3 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tab bar
            Constraint::Min(0),    // active pane body
        ])
        .split(inner);

    // Tab bar listing the child sub-panes; the active one is highlighted.
    let tabs: [(&str, crate::app_shell::nav::IwPane); 2] = [
        ("Overview", crate::app_shell::nav::IwPane::Overview),
        ("Connections", crate::app_shell::nav::IwPane::Connections),
    ];
    let mut spans = Vec::new();
    for (idx, (label, pane)) in tabs.iter().enumerate() {
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

    // Body shows only the active sub-pane (like a tab page).
    match state.pane {
        crate::app_shell::nav::IwPane::Overview => {
            overview_view::render(frame, theme, chunks[1], &state.overview)
        }
        crate::app_shell::nav::IwPane::Connections => {
            connections_view::render(frame, theme, chunks[1], &state.connections, focused)
        }
    }
}