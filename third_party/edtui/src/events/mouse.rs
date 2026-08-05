use crossterm::event::{MouseEvent as CTMouseEvent, MouseEventKind};
use jagged::Index2;

use crate::{
    actions::{Execute, SwitchMode},
    helper::char_width,
    state::selection::set_selection,
    state::{logical_pos_at_visual_row, total_visual_rows, visual_row_of_cursor},
    view::line_wrapper::LineWrapper,
    EditorMode, EditorState,
};

/// The number of lines to scroll per scroll wheel event.
const SCROLL_LINES: usize = 1;

/// Handles a mouse event.
#[derive(Clone, Debug, Default)]
pub struct MouseEventHandler {}

impl MouseEventHandler {
    pub fn on_event<E>(event: E, state: &mut EditorState)
    where
        E: Into<MouseEvent>,
    {
        let event = event.into();
        if event == MouseEvent::None {
            return;
        }

        // Handle scroll events
        match event {
            MouseEvent::ScrollUp(mouse) => {
                if Self::is_position_within_bounds(&mouse, state) {
                    Self::handle_scroll_up(state);
                }
                return;
            }
            MouseEvent::ScrollDown(mouse) => {
                if Self::is_position_within_bounds(&mouse, state) {
                    Self::handle_scroll_down(state);
                }
                return;
            }
            _ => {}
        }

        // Check if the mouse event is within the editor's screen area
        if !Self::is_within_bounds(&event, state) {
            return;
        }

        if let MouseEvent::Down(_) = event {
            state.selection = None;
            if state.mode == EditorMode::Visual {
                SwitchMode(EditorMode::Normal).execute(state);
            }
        }

        if let MouseEvent::Drag(_) = event {
            if state.mode != EditorMode::Visual {
                SwitchMode(EditorMode::Visual).execute(state);
            }
            set_selection(&mut state.selection, state.cursor);
        }

        match event {
            MouseEvent::Down(mouse) | MouseEvent::Up(mouse) | MouseEvent::Drag(mouse) => {
                let lines = &state.lines;
                let cursor = mouse_position_to_cursor_position(state, &mouse, state.view.tab_width);
                let last_row = lines.last_row_index();
                let last_col = lines.last_col_index(cursor.row);

                // row is out of bounds
                if last_row < cursor.row {
                    let last_col = lines.last_col_index(last_row);
                    state.cursor = Index2::new(last_row, last_col);
                // col is out of bounds
                } else if last_col < cursor.col {
                    state.cursor = Index2::new(cursor.row, last_col);
                } else {
                    state.cursor = cursor;
                }

                if let MouseEvent::Drag(_) = event {
                    set_selection(&mut state.selection, state.cursor);
                }
            }
            MouseEvent::ScrollUp(_) | MouseEvent::ScrollDown(_) | MouseEvent::None => (),
        };
    }

    fn handle_scroll_up(state: &mut EditorState) {
        state.view.viewport.y = state.view.viewport.y.saturating_sub(SCROLL_LINES);
        Self::clamp_cursor_to_viewport(state);
    }

    fn handle_scroll_down(state: &mut EditorState) {
        let height = state.view.screen_area.height as usize;
        let width = state.view.screen_area.width as usize;
        let tab_width = state.view.tab_width;
        let max_viewport_y = if state.view.wrap {
            total_visual_rows(&state.lines, width, tab_width).saturating_sub(height.max(1))
        } else {
            state.lines.len().saturating_sub(1)
        };
        if state.view.viewport.y >= max_viewport_y {
            return;
        }
        state.view.viewport.y = (state.view.viewport.y + SCROLL_LINES).min(max_viewport_y);
        Self::clamp_cursor_to_viewport(state);
    }

    fn clamp_cursor_to_viewport(state: &mut EditorState) {
        if state.view.wrap {
            Self::clamp_cursor_to_wrap_viewport(state);
            return;
        }

        let viewport_y = state.view.viewport.y;
        let viewport_height = state.view.num_rows;

        if viewport_height == 0 {
            return;
        }

        let viewport_bottom = viewport_y + viewport_height.saturating_sub(1);

        if state.cursor.row < viewport_y {
            state.cursor.row = viewport_y;
            state.clamp_column();
        } else if state.cursor.row > viewport_bottom {
            state.cursor.row = viewport_bottom.min(state.lines.last_row_index());
            state.clamp_column();
        }
    }

    fn clamp_cursor_to_wrap_viewport(state: &mut EditorState) {
        let width = state.view.screen_area.width as usize;
        let height = state.view.screen_area.height as usize;
        let tab_width = state.view.tab_width;
        if width == 0 || height == 0 {
            return;
        }

        let cursor_visual = visual_row_of_cursor(
            &state.lines,
            width,
            tab_width,
            state.cursor.row,
            state.cursor.col,
        );
        let top = state.view.viewport.y;
        let bottom = top + height.saturating_sub(1);
        if cursor_visual >= top && cursor_visual <= bottom {
            return;
        }

        let target_visual = if cursor_visual < top { top } else { bottom };
        let (row, seg) = logical_pos_at_visual_row(&state.lines, width, tab_width, target_visual);
        let line = state.lines.iter_row().nth(row).cloned().unwrap_or_default();
        let wrapped = LineWrapper::wrap_line(&line, width, tab_width);
        let segments = if wrapped.is_empty() {
            vec![Vec::new()]
        } else {
            wrapped
        };
        let col: usize = segments.iter().take(seg).map(Vec::len).sum();
        state.cursor = Index2::new(row, col);
        state.clamp_column();
    }

    /// Checks if the mouse event occurred within the editor's screen area.
    fn is_within_bounds(event: &MouseEvent, state: &EditorState) -> bool {
        let mouse = match event {
            MouseEvent::Down(pos) | MouseEvent::Up(pos) | MouseEvent::Drag(pos) => pos,
            MouseEvent::ScrollUp(pos) | MouseEvent::ScrollDown(pos) => pos,
            MouseEvent::None => return false,
        };

        Self::is_position_within_bounds(mouse, state)
    }

    fn is_position_within_bounds(mouse: &MousePosition, state: &EditorState) -> bool {
        let area = &state.view.screen_area;
        let x: usize = area.x.into();
        let y: usize = area.y.into();
        let width: usize = area.width.into();
        let height: usize = area.height.into();

        mouse.col >= x && mouse.col < x + width && mouse.row >= y && mouse.row < y + height
    }
}

fn mouse_position_to_cursor_position(
    state: &EditorState,
    mouse: &MousePosition,
    tab_width: usize,
) -> Index2 {
    // Global -> editor coordinates
    let mouse = Index2::new(
        mouse.row.saturating_sub(state.view.screen_area.y.into()),
        mouse.col.saturating_sub(state.view.screen_area.x.into()),
    );

    if !state.view.wrap {
        return Index2::new(
            mouse.row.saturating_add(state.view.viewport.y),
            mouse.col.saturating_add(state.view.viewport.x),
        );
    }

    let width = state.view.screen_area.width as usize;
    // viewport.y is absolute visual row; map click to that + mouse.row within pane.
    let absolute_visual = state.view.viewport.y.saturating_add(mouse.row);
    let (logical_row, seg) =
        logical_pos_at_visual_row(&state.lines, width, tab_width, absolute_visual);
    let line = state
        .lines
        .iter_row()
        .nth(logical_row)
        .cloned()
        .unwrap_or_default();
    let wrapped = LineWrapper::wrap_line(&line, width, tab_width);
    let segments = if wrapped.is_empty() {
        vec![Vec::new()]
    } else {
        wrapped
    };
    let Some(segment) = segments.get(seg) else {
        return Index2::new(logical_row, 0);
    };
    let col_offset: usize = segments.iter().take(seg).map(Vec::len).sum();
    let mut current_width = 0usize;
    let mut col_in_seg = 0usize;
    for &ch in segment {
        let cw = char_width(ch, tab_width);
        if current_width + cw > mouse.col {
            break;
        }
        current_width += cw;
        col_in_seg += 1;
    }
    Index2::new(logical_row, col_offset + col_in_seg)
}

/// Represents a mouse event.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum MouseEvent {
    /// A mouse press event.
    Down(MousePosition),

    /// A mouse release event.
    Up(MousePosition),

    /// A mouse Drag event.
    Drag(MousePosition),

    /// A scroll up (wheel up) event.
    ScrollUp(MousePosition),

    /// A scroll down (wheel down) event.
    ScrollDown(MousePosition),

    /// A mouse event that is not handled by the editor.
    None,
}

impl From<CTMouseEvent> for MouseEvent {
    fn from(event: CTMouseEvent) -> Self {
        match event.kind {
            MouseEventKind::Down(_) => Self::Down(MousePosition::new(event.row, event.column)),
            MouseEventKind::Up(_) => Self::Up(MousePosition::new(event.row, event.column)),
            MouseEventKind::Drag(_) => Self::Drag(MousePosition::new(event.row, event.column)),
            MouseEventKind::ScrollUp => Self::ScrollUp(MousePosition::new(event.row, event.column)),
            MouseEventKind::ScrollDown => {
                Self::ScrollDown(MousePosition::new(event.row, event.column))
            }
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct MousePosition {
    /// The row that the event occurred on.
    pub(crate) row: usize,
    /// The column that the event occurred on.
    pub(crate) col: usize,
}

impl MousePosition {
    /// Creates a new `MousePosition` instance.
    fn new(row: u16, col: u16) -> Self {
        Self {
            row: row.into(),
            col: col.into(),
        }
    }
}
