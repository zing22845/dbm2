//! SQL completion sub-module rendering (the completion popup).
//!
//! Draws a bordered popup with the ranked items; the selected item is
//! highlighted. The popup is anchored within the editor area and renders
//! nothing when closed.

use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::SqlCompletionState;
use super::provider::{CompletionItem, CompletionKind};

const MAX_VISIBLE: usize = 8;
const POPUP_WIDTH: u16 = 48;

/// Entry point used by the editor: draws the popup anchored to the editor
/// cursor. `cursor_pos` is the caret's absolute terminal position (from
/// `render_editor`); the popup is placed one row below the caret so it follows
/// the caret as the user types, matching the original dbm. Falls back to the
/// top-left of `area` when no caret position is available. Renders nothing when
/// closed.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &SqlCompletionState,
    cursor_pos: Option<Position>,
) {
    let anchor = cursor_pos.map_or_else(
        || Position::new(area.x, area.y),
        |p| Position::new(p.x, p.y.saturating_add(1)),
    );
    draw_completion_popup_at(frame, theme, area, anchor, state);
}

/// Height of the popup (0 when closed).
pub fn popup_height(state: &SqlCompletionState) -> u16 {
    if !state.is_open() {
        0
    } else {
        state.items.len().min(MAX_VISIBLE) as u16 + 2
    }
}

/// Draw the completion popup anchored at `anchor` within `editor_area`.
/// Returns the popup rect (or an empty rect when nothing is drawn).
pub fn draw_completion_popup_at(
    frame: &mut Frame,
    theme: &Theme,
    editor_area: Rect,
    anchor: Position,
    state: &SqlCompletionState,
) -> Rect {
    if editor_area.width < 4 || editor_area.height < 3 || !state.is_open() {
        return Rect::default();
    }

    let popup_w = POPUP_WIDTH.min(editor_area.width);
    let popup_h = popup_height(state).min(editor_area.height.saturating_sub(1));
    if popup_h < 3 {
        return Rect::default();
    }

    let editor_right = editor_area.x.saturating_add(editor_area.width);
    let editor_bottom = editor_area.y.saturating_add(editor_area.height);
    let x = anchor
        .x
        .max(editor_area.x)
        .min(editor_right.saturating_sub(popup_w));
    let mut y = anchor.y;
    if y.saturating_add(popup_h) > editor_bottom {
        y = anchor.y.saturating_sub(popup_h.saturating_add(1));
    }
    y = y
        .max(editor_area.y)
        .min(editor_bottom.saturating_sub(popup_h));

    let popup = Rect {
        x,
        y,
        width: popup_w,
        height: popup_h,
    };

    let p = theme.palette();
    let block = Block::default()
        .title(" Complete ")
        .borders(Borders::ALL)
        .border_style(p.popup_border(true))
        .style(Style::default().bg(p.surface));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let visible = state.items.len().min(MAX_VISIBLE);
    let start = state.selected.saturating_sub(visible.saturating_sub(1));
    let lines: Vec<Line> = state
        .items
        .iter()
        .skip(start)
        .take(visible)
        .enumerate()
        .map(|(row, item)| completion_line(item, start + row == state.selected, p.selection_text, p.selection_bg, p.muted))
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
    popup
}

fn completion_line(
    item: &CompletionItem,
    selected: bool,
    selection_fg: ratatui::style::Color,
    selection_bg: ratatui::style::Color,
    muted: ratatui::style::Color,
) -> Line<'static> {
    let tag = match item.kind {
        CompletionKind::Keyword => "[K]",
        CompletionKind::Table => "[T]",
        CompletionKind::Column => "[C]",
        CompletionKind::Alias => "[A]",
        CompletionKind::Schema => "[S]",
    };
    let base = if selected {
        Style::default()
            .fg(selection_fg)
            .bg(selection_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let mut spans = vec![
        Span::styled(format!("{tag} "), base),
        Span::styled(item.label.clone(), base),
    ];
    if let Some(detail) = &item.detail {
        spans.push(Span::styled(
            format!(" · {detail}"),
            Style::default().fg(muted),
        ));
    }
    Line::from(spans)
}
