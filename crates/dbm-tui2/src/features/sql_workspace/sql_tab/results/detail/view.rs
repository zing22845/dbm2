//! Results detail sub-module rendering (read-only cell body preview).

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::components::line_numbers;
use crate::common::utils::text_width;
use crate::common::view::theme::Theme;

use super::state::DetailState;

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

/// Render the read-only cell body preview. `focused` toggles the border accent.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &DetailState,
    body: &str,
    title: String,
    focused: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = theme.palette();
    let border = if focused {
        Style::default().fg(p.border_active)
    } else {
        Style::default().fg(p.border)
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border)
        .style(Style::default().bg(p.surface));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let viewport = inner.height as usize;
    let display_lines = build_detail_lines(body, inner.width);
    let mut detail_state = state.clone();
    detail_state.clamp_scroll(display_lines.len(), viewport);
    let visible: Vec<Line> = display_lines
        .into_iter()
        .skip(detail_state.scroll)
        .take(viewport.max(1))
        .collect();
    frame.render_widget(Paragraph::new(visible), inner);
}
