//! Explorer objects feature rendering.

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::ObjectsState;

/// Render the object tree. Objects are not yet migrated; the pane renders as an
/// empty placeholder.
pub fn render(_frame: &mut Frame, _theme: &Theme, _area: Rect, _state: &ObjectsState) {}
