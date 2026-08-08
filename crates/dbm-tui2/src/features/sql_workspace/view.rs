//! SQL workspace feature rendering.
//!
//! The SQL workspace is a parent pane: an outer " SQL Workspace " border wraps
//! the active tab (its tab bar plus editor / history / results), mirroring the
//! original dbm's `draw_workspace` block. `focused` draws the outer border as
//! the active one.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::SqlState;
use super::sql_tab::view as sql_tab_view;

/// Render the SQL workspace parent pane: an outer " SQL Workspace " border
/// wrapping the active tab's content (delegated to the `sql_tab` renderer).
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &SqlState,
    focused: bool,
) {
    let p = theme.palette();
    let border_color = if focused { p.border_active } else { p.border };
    let outer = Block::default()
        .title(" SQL Workspace ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    sql_tab_view::render(frame, theme, inner, &state.sql_tab);
}
