//! SQL workspace feature rendering.

use ratatui::layout::Rect;
use ratatui::Frame;

use super::state::SqlState;
use super::sql_tab::view as sql_tab_view;

/// Render the SQL workspace. Delegates to the `sql_tab` child renderer.
pub fn render(frame: &mut Frame, area: Rect, state: &SqlState) {
    sql_tab_view::render(frame, area, &state.sql_tab);
}
