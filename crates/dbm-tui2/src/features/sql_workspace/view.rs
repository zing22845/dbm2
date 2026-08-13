//! SQL workspace feature rendering.
//!
//! The SQL workspace is a parent pane: an outer " SQL Workspace " border wraps
//! the active tab (its tab bar plus editor / history / results), mirroring the
//! original dbm's `draw_workspace` block. `focused` draws the outer border as
//! the active one. A workspace-level footer hint (tab management) sits at the
//! bottom inside the border, distinct from any single pane's footer.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::common::view::hints::{draw_footer, footer_height, sql_workspace_footer_text};
use crate::common::view::theme::Theme;

use super::state::SqlState;
use super::sql_tab::view as sql_tab_view;

/// Render the SQL workspace parent pane: an outer " SQL Workspace " border
/// wrapping the active tab's content (delegated to the `sql_tab` renderer),
/// with a workspace-level tab-management footer below it.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &SqlState,
    focused: bool,
) {
    let p = theme.palette();
    let outer = Block::default()
        .title(" SQL Workspace ")
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    // Tab management (close / switch / open tab) is a workspace concern, so its
    // hint belongs here, not in any single pane's footer. It is only shown when
    // there is at least one open tab.
    if !state.sql_tab.tabs.is_empty() {
        let hint = sql_workspace_footer_text();
        let footer_h = footer_height(&hint, inner.width).min(inner.height.saturating_sub(1));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(footer_h)])
            .split(inner);
        sql_tab_view::render(frame, theme, chunks[0], &state.sql_tab);
        draw_footer(frame, theme, chunks[1], &hint);
    } else {
        // No tabs: let sql_tab render its empty-state hint over the full area.
        sql_tab_view::render(frame, theme, inner, &state.sql_tab);
    }
}
