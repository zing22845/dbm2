use crate::{
    helper::char_width,
    view::line_wrapper::LineWrapper,
    view::LineNumbers,
    Lines,
};
use ratatui_core::layout::{Position, Rect};

/// Represents the (x, y) offset of the editor's viewport.
/// It represents the top-left local editor coordinate.
#[derive(Debug, Clone)]
pub(crate) struct ViewState {
    /// The offset of the viewport.
    ///
    /// - Nowrap: `y` is the first visible **logical** line index; `x` is the first
    ///   visible character column.
    /// - Wrap: `y` is the first visible **visual** (wrapped) row index across the
    ///   whole buffer; `x` is unused (kept at 0).
    pub(crate) viewport: Offset,
    /// The number of rows that are displayed on the viewport
    /// (logical lines when nowrap; visual rows when wrap).
    pub(crate) num_rows: usize,
    /// Sets the area (starting upper-left corner of the terminal window) where
    /// the editor text is rendered to.
    ///
    /// Required to calculate the mouse position in relation to the text within the editor.
    pub(crate) screen_area: Rect,
    /// Whether the lines are wrapped.
    pub(crate) wrap: bool,
    /// The number of spaces used to display a tab.
    pub(crate) tab_width: usize,
    /// Line numbers configuration.
    pub(crate) line_numbers: LineNumbers,
    /// The cursor's screen position, computed during the last render.
    /// This is the absolute position in terminal coordinates where the cursor should be displayed.
    pub(crate) cursor_screen_position: Option<Position>,
    /// Whether the editor is in single-line mode (blocks newline insertion).
    pub(crate) single_line: bool,
    /// When `true`, `update_viewport_vertical{,_wrap}` and
    /// `update_viewport_horizontal` skip the cursor-following adjustment
    /// (they still clamp to `max_offset`). Set by external code that
    /// performs manual scrollbar drags or wheel scrolling — the user has
    /// intentionally scrolled away from the cursor and we must not snap
    /// back during the next render. Re-engaged by the caller when editing
    /// resumes (keyboard input / paste / buffer replacement).
    pub(crate) scroll_locked: bool,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            viewport: Offset::default(),
            num_rows: 0,
            screen_area: Rect::default(),
            wrap: true,
            tab_width: 2,
            line_numbers: LineNumbers::None,
            cursor_screen_position: None,
            single_line: false,
            scroll_locked: false,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Hash)]
pub(crate) struct Offset {
    /// The x-offset.
    pub(crate) x: usize,
    /// The y-offset.
    pub(crate) y: usize,
}

impl Offset {
    pub(crate) fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }
}

impl From<Rect> for Offset {
    fn from(value: Rect) -> Self {
        Self {
            x: value.x as usize,
            y: value.y as usize,
        }
    }
}

impl ViewState {
    /// Sets the editors area on the screen.
    ///
    /// Equivalent to the upper left coordinate of the editor in the
    /// buffers coordinate system.
    pub(crate) fn set_screen_area<T: Into<Rect>>(&mut self, area: T) {
        self.screen_area = area.into();
    }

    /// Updates the viewports horizontal offset.
    pub(crate) fn update_viewport_horizontal(
        &mut self,
        width: usize,
        cursor_col: usize,
        line: Option<&Vec<char>>,
    ) -> usize {
        let Some(line) = line else {
            self.viewport.x = 0;
            return self.viewport.x;
        };

        if self.scroll_locked {
            return self.viewport.x;
        }

        // scroll left
        if cursor_col < self.viewport.x {
            self.viewport.x = cursor_col;
            return self.viewport.x;
        }

        // Iterate forward from the viewport.x position and calculate width
        let mut max_cursor_pos = self.viewport.x;
        let mut current_width = 0;
        for &ch in line.iter().skip(self.viewport.x) {
            current_width += char_width(ch, self.tab_width);
            if current_width >= width {
                break;
            }
            max_cursor_pos += 1;
        }

        // scroll right
        if cursor_col > max_cursor_pos {
            let mut backward_width = 0;
            let mut new_viewport_x = cursor_col;

            // Iterate backward from max_cursor_pos to find the first fitting character
            for i in (0..=cursor_col).rev() {
                let char_width = match line.get(i) {
                    Some(&ch) => char_width(ch, self.tab_width),
                    None => 1,
                };
                backward_width += char_width;
                if backward_width >= width {
                    break;
                }
                new_viewport_x = new_viewport_x.saturating_sub(1);
            }

            self.viewport.x = new_viewport_x;
        }

        self.viewport.x
    }

    /// Updates the view ports vertical offset.
    pub(crate) fn update_viewport_vertical(&mut self, height: usize, cursor_row: usize) -> usize {
        if !self.scroll_locked {
            let max_cursor_pos = height.saturating_sub(1) + self.viewport.y;

            // scroll up
            if cursor_row < self.viewport.y {
                self.viewport.y = cursor_row;
            }

            // scroll down
            if cursor_row >= max_cursor_pos {
                self.viewport.y += cursor_row.saturating_sub(max_cursor_pos);
            }
        }

        self.viewport.y
    }

    /// Keep the cursor's wrapped visual row inside the viewport.
    ///
    /// Unlike logical-line-only scroll, a single long line that wraps past
    /// `height` can still advance `viewport.y` (visual rows).
    pub(crate) fn update_viewport_vertical_wrap(
        &mut self,
        width: usize,
        height: usize,
        cursor_row: usize,
        cursor_col: usize,
        lines: &Lines,
    ) -> usize {
        if height == 0 || width == 0 {
            return self.viewport.y;
        }

        let cursor_visual =
            visual_row_of_cursor(lines, width, self.tab_width, cursor_row, cursor_col);
        let total_visual = total_visual_rows(lines, width, self.tab_width);
        let max_offset = total_visual.saturating_sub(height);
        self.viewport.y = self.viewport.y.min(max_offset);

        if !self.scroll_locked {
            if cursor_visual < self.viewport.y {
                self.viewport.y = cursor_visual;
            } else {
                let bottom = self.viewport.y + height.saturating_sub(1);
                if cursor_visual > bottom {
                    self.viewport.y = cursor_visual.saturating_sub(height.saturating_sub(1));
                }
            }
        }

        self.viewport.y = self.viewport.y.min(max_offset);
        self.viewport.y
    }

    /// Updates the number of rows that are currently shown on the viewport.
    pub(crate) fn update_num_rows(&mut self, num_rows: usize) {
        self.num_rows = num_rows;
    }
}

/// Number of visual rows occupied by one logical line at `width`.
pub(crate) fn visual_row_count_for_line(line: &[char], width: usize, tab_width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    LineWrapper::wrap_line(line, width, tab_width)
        .len()
        .max(1)
}

/// Total visual rows in the buffer when wrapping at `width`.
pub(crate) fn total_visual_rows(lines: &Lines, width: usize, tab_width: usize) -> usize {
    if lines.is_empty() {
        return 1;
    }
    lines
        .iter_row()
        .map(|line| visual_row_count_for_line(line, width, tab_width))
        .sum()
}

/// Absolute visual row index of the cursor (0-based) when wrapping at `width`.
pub(crate) fn visual_row_of_cursor(
    lines: &Lines,
    width: usize,
    tab_width: usize,
    cursor_row: usize,
    cursor_col: usize,
) -> usize {
    if width == 0 {
        return cursor_row;
    }
    let mut visual = 0usize;
    for (i, line) in lines.iter_row().enumerate() {
        let wrapped = LineWrapper::wrap_line(line, width, tab_width);
        let segments = if wrapped.is_empty() {
            vec![Vec::new()]
        } else {
            wrapped
        };
        if i < cursor_row {
            visual += segments.len();
            continue;
        }
        if i > cursor_row {
            break;
        }
        let mut consumed = 0usize;
        for (seg_i, seg) in segments.iter().enumerate() {
            let seg_len = seg.len();
            let last = seg_i + 1 == segments.len();
            // Cursor on this segment if inside it, or at EOL on the last segment.
            if cursor_col < consumed + seg_len || (last && cursor_col >= consumed) {
                return visual + seg_i;
            }
            consumed += seg_len;
        }
        return visual;
    }
    visual
}

/// Map an absolute visual row to `(logical_row, wrap_segment_index)`.
pub(crate) fn logical_pos_at_visual_row(
    lines: &Lines,
    width: usize,
    tab_width: usize,
    visual_row: usize,
) -> (usize, usize) {
    if lines.is_empty() || width == 0 {
        return (0, 0);
    }
    let mut remaining = visual_row;
    let last_row = lines.len().saturating_sub(1);
    for (i, line) in lines.iter_row().enumerate() {
        let n = visual_row_count_for_line(line, width, tab_width);
        if remaining < n {
            return (i, remaining);
        }
        remaining -= n;
        if i == last_row {
            return (i, n.saturating_sub(1));
        }
    }
    (last_row, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! update_view_vertical_test {
        ($name:ident: {
        view: $view:expr,
        height: $height:expr,
        cursor: $cursor:expr,
        expected: $expected:expr
    }) => {
            #[test]
            fn $name() {
                // given
                let mut view = $view;

                // when
                let offset = view.update_viewport_vertical($height, $cursor);

                // then
                assert_eq!(offset, $expected);
            }
        };
    }

    macro_rules! update_view_horizontal_test {
        ($name:ident: {
        view: $view:expr,
        width: $width:expr,
        cursor: $cursor:expr,
        expected: $expected:expr
    }) => {
            #[test]
            fn $name() {
                // given
                let mut view = $view;
                let line = vec![];

                // when
                let offset = view.update_viewport_horizontal($width, $cursor, Some(&line));

                // then
                assert_eq!(offset, $expected);
            }
        };
    }

    // cursor above viewport → scroll up
    update_view_vertical_test!(
        scroll_up: {
            view: ViewState{
                viewport: Offset::new(0, 1),
                ..Default::default()
            },
            height:  2,
            cursor: 0,
            expected: 0
        }
    );

    // cursor below viewport → scroll down
    update_view_vertical_test!(
        scroll_down: {
            view: ViewState{
                viewport: Offset::new(0, 0),
                ..Default::default()
            },
            height:  2,
            cursor: 2,
            expected: 1
        }
    );

    // cursor left of viewport → scroll left
    update_view_horizontal_test!(
        scroll_left: {
            view: ViewState{
                viewport: Offset::new(1, 0),
                ..Default::default()
            },
            width: 2,
            cursor: 0,
            expected: 0
        }
    );

    // cursor right of viewport → scroll right
    update_view_horizontal_test!(
        scroll_right: {
            view: ViewState{
                viewport: Offset::new(0, 0),
                ..Default::default()
            },
            width: 2,
            cursor: 2,
            expected: 1
        }
    );

    #[test]
    fn wrap_scroll_advances_within_single_long_line() {
        let lines = Lines::from("abcdefghijklmnopqrstuvwxyz");
        let mut view = ViewState {
            viewport: Offset::new(0, 0),
            ..Default::default()
        };
        // width 5 → many visual rows; cursor on char 20 → visual row 4
        let offset = view.update_viewport_vertical_wrap(5, 2, 0, 20, &lines);
        assert_eq!(offset, 3); // keep cursor on last visible row (visual 4 → offset 3)
    }
}
