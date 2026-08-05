//! SQL completion sub-module rendering.

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::SqlCompletionState;

pub fn render(_frame: &mut Frame, _theme: &Theme, _area: Rect, _state: &SqlCompletionState) {}
