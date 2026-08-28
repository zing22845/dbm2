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
use crate::app::state::SplitterHoverState;

/// The outer block title for the SQL workspace, showing the active connection
/// (matching the original dbm's `workspace_block_title_from_tree`): when a
/// connection is active, ` Connection · {connection} @ {instance} `; otherwise
/// a plain ` Workspace `.
fn workspace_title(state: &SqlState) -> String {
    match state.sql_tab.active_connection() {
        Some((instance, connection)) => {
            format!(" Connection · {connection} @ {instance} ")
        }
        None => " Workspace ".to_string(),
    }
}

/// Render the SQL workspace parent pane: an outer " SQL Workspace " border
/// wrapping the active tab's content (delegated to the `sql_tab` renderer),
/// with a workspace-level tab-management footer below it. Returns the editor's
/// hardware cursor when the active tab's editor sub-pane holds focus.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &SqlState,
    focused: bool,
    splitter_hover: &SplitterHoverState,
) -> (Option<crate::common::editor::EditorHardwareCursor>, Option<usize>) {
    let p = theme.palette();
    let outer = Block::default()
        .title(workspace_title(state))
        .borders(Borders::ALL)
        .border_style(p.active_border(focused));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    // Tab management (close / switch / open tab) is a workspace concern, so its
    // hint belongs here, not in any single pane's footer. It is only shown when
    // the active connection has at least one open tab.
    if !state.sql_tab.active_connection_is_empty() {
        let hint = sql_workspace_footer_text();
        let footer_h = footer_height(&hint, inner.width).min(inner.height.saturating_sub(1));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(footer_h)])
            .split(inner);
        let (cursor, v_scroll) = sql_tab_view::render(
            frame,
            theme,
            chunks[0],
            &state.sql_tab,
            focused,
            splitter_hover.sql_editor_results,
            splitter_hover.sql_editor_history,
            splitter_hover.history_detail,
            splitter_hover.sql_editor_results_drag,
            splitter_hover.sql_editor_history_drag,
            splitter_hover.sql_history_detail_drag,
            splitter_hover.results_detail,
            splitter_hover.sql_results_detail_drag,
        );
        draw_footer(frame, theme, chunks[1], &hint);
        (cursor, v_scroll)
    } else {
        // Active connection has no visible tab: let sql_tab render its empty
        // state hint over the full area.
        sql_tab_view::render(
            frame,
            theme,
            inner,
            &state.sql_tab,
            focused,
            splitter_hover.sql_editor_results,
            splitter_hover.sql_editor_history,
            splitter_hover.history_detail,
            splitter_hover.sql_editor_results_drag,
            splitter_hover.sql_editor_history_drag,
            splitter_hover.sql_history_detail_drag,
            splitter_hover.results_detail,
            splitter_hover.sql_results_detail_drag,
        )
    }
}
