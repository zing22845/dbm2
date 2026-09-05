//! Results detail sub-module rendering: read-only cell body preview with
//! optional action buttons (Save/Discard when editing) and a detail footer.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::common::components::line_numbers;
use crate::common::utils::text_width;
use crate::common::view::hints::{draw_footer, footer_height};
use crate::common::view::theme::Theme;

use super::state::DetailState;

/// Height of the detail action buttons row (when editing).
const DETAIL_ACTION_BTNS_HEIGHT: u16 = 1;

fn wrap_plain_line(line: &str, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from(line.to_string())];
    }
    if line.is_empty() {
        return vec![Line::from("")];
    }
    let mut rows = Vec::new();
    let mut current = String::new();
    let mut used = 0usize;
    for ch in line.chars() {
        let cw = text_width::char_width(ch).max(1);
        if used > 0 && used + cw > width {
            rows.push(Line::from(std::mem::take(&mut current)));
            used = 0;
        }
        current.push(ch);
        used += cw;
    }
    rows.push(Line::from(current));
    rows
}

/// Display rows of `body` when wrapped at `text_width` (with gutter).
pub fn detail_display_line_count(body: &str, text_width: u16) -> usize {
    if body.is_empty() {
        return 1;
    }
    let logical = body.lines().count().max(1);
    let w = line_numbers::text_width_after_gutter(text_width, logical) as usize;
    body.lines().map(|line| wrap_plain_line(line, w).len()).sum()
}

fn build_detail_lines(body: &str, text_width: u16) -> Vec<Line<'static>> {
    if body.is_empty() {
        let gutter_w = line_numbers::gutter_width(1);
        return vec![Line::from(vec![
            Span::styled(
                line_numbers::format_gutter(1, gutter_w),
                line_numbers::gutter_style(),
            ),
            Span::raw(""),
        ])];
    }
    let logical: Vec<&str> = body.lines().collect();
    let gutter_w = line_numbers::gutter_width(logical.len().max(1));
    let w = text_width.saturating_sub(gutter_w).max(1);
    let mut out = Vec::new();
    for (i, line) in logical.iter().enumerate() {
        let wrapped = wrap_plain_line(line, w as usize);
        out.extend(line_numbers::prefix_wrapped_line(i + 1, gutter_w, wrapped));
    }
    out
}

/// Build the detail footer hint text.
fn detail_footer_text(detail: &DetailState, edit_active: bool) -> String {
    if edit_active {
        if detail.dirty {
            " Detail  | Ctrl-S:save  Ctrl-D:discard  Esc:leave"
        } else {
            " Detail  | [edit mode]  Esc:leave"
        }
    } else {
        " Detail  | Back: Esc"
    }
    .to_string()
}

/// Build the detail action buttons line (Save/Discard when editing).
fn detail_action_buttons_line(detail: &DetailState, edit_active: bool) -> Option<Line<'static>> {
    if !edit_active || !detail.dirty {
        return None;
    }
    let p = String::from(" [Ctrl-S] Save   [Ctrl-D] Discard");
    Some(Line::from(vec![
        Span::styled(
            p,
            Style::default().fg(Color::Yellow),
        ),
    ]))
}

/// Render the detail sub-pane with its own border, optional action buttons,
/// body area with scroll, and a detail footer.
///
/// `edit_active` is passed from the parent (list state) since edit mode is
/// owned by the list sub-feature.
#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    detail: &DetailState,
    body: &str,
    title: String,
    edit_active: bool,
    focused: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = theme.palette();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    // Title line inside the outer Results block — no own border.
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            title,
            Style::default().fg(if focused { p.accent } else { p.muted }),
        ))),
        chunks[0],
    );

    let inner = chunks[1];

    let has_action_btns = edit_active && detail.dirty;
    let footer_h = footer_height(&detail_footer_text(detail, edit_active), inner.width).min(3);

    let chunks = if has_action_btns {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(DETAIL_ACTION_BTNS_HEIGHT),
                Constraint::Min(0),
                Constraint::Length(footer_h),
            ])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(footer_h),
            ])
            .split(inner)
    };

    let body_area = if has_action_btns { chunks[1] } else { chunks[0] };
    let footer_area = if has_action_btns { chunks[2] } else { chunks[1] };

    // Detail action buttons (only when editing and dirty).
    if has_action_btns
        && let Some(line) = detail_action_buttons_line(detail, edit_active)
    {
        frame.render_widget(Paragraph::new(line), chunks[0]);
    }

    // Detail body.
    let viewport = body_area.height as usize;
    let display_lines = build_detail_lines(body, body_area.width);
    let lines_total = display_lines.len();
    let mut detail_state = detail.clone();
    detail_state.clamp_scroll(lines_total, viewport);
    let visible: Vec<Line> = display_lines
        .into_iter()
        .skip(detail_state.scroll)
        .take(viewport.max(1))
        .collect();
    frame.render_widget(Paragraph::new(visible), body_area);

    // Detail footer.
    let hint = detail_footer_text(detail, edit_active);
    draw_footer(frame, theme, footer_area, &hint);
}