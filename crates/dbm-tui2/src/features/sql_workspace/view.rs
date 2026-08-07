//! SQL workspace feature rendering.

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::SqlState;
use super::sql_tab::view as sql_tab_view;

/// Render the SQL workspace. Delegates to the `sql_tab` child renderer.
/// `focused` controls whether the workspace border is drawn as the active one.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &SqlState,
    focused: bool,
) {
    sql_tab_view::render(frame, theme, area, &state.sql_tab, focused);
}
