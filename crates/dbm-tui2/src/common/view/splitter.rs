//! Shared pane-splitter drawing and ratio helpers.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Paragraph;

const SPLITTER_LINE_DIM: Style = Style::new().fg(Color::Rgb(55, 55, 60));
/// The shared hover color for any resizable separator: used both by the pane
/// splitters and by the results column-width resize handle, so hover feedback
/// reads consistently across the UI and stays a single point of change.
pub const SPLITTER_LINE_HOVER: Style = Style::new().fg(Color::Cyan);
const SPLITTER_LINE_DRAG: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitOrientation {
    Horizontal,
    Vertical,
}

fn line_style(hover: bool, dragging: bool) -> Style {
    if dragging {
        SPLITTER_LINE_DRAG
    } else if hover {
        SPLITTER_LINE_HOVER
    } else {
        SPLITTER_LINE_DIM
    }
}

pub fn draw(
    frame: &mut Frame,
    area: Rect,
    orientation: SplitOrientation,
    hover: bool,
    dragging: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let style = line_style(hover, dragging);
    let text = match orientation {
        SplitOrientation::Horizontal => "─".repeat(area.width as usize),
        SplitOrientation::Vertical => (0..area.height).map(|_| "│").collect::<Vec<_>>().join("\n"),
    };
    frame.render_widget(Paragraph::new(text).style(style), area);
}
