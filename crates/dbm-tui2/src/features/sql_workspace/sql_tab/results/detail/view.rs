//! Results detail sub-module rendering.
//!
//! The detail is its own bordered panel (mirroring the original dbm's
//! `draw_results_detail`): the border doubles as the focus cue — bright while
//! the detail cell editor holds focus, dim otherwise — and its title carries
//! the cell reference plus, while focused, the current editor mode.
//!
//! Two body modes:
//!  * **Read-only preview**: shows the selected cell's value with line numbers,
//!    following the table selection. Used while the detail editor is not
//!    focused (focus returned to the table).
//!  * **Cell editor** (`detail.focused`): embeds the edtui editor over the
//!    draft (with baseline-diff highlights). Save / Discard action row and
//!    footer appear while the draft is dirty.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::common::components::line_numbers;
use crate::common::layout::text::footer_height;
use crate::common::utils::text_width;
use crate::common::view::hints::draw_footer;
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
    body.lines()
        .map(|line| wrap_plain_line(line, w).len())
        .sum()
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

/// The active editor mode label, when the detail editor is focused.
fn editor_mode_label(detail: &DetailState) -> &'static str {
    let Some(mode) = detail.editor.as_ref().map(|h| h.editor.mode) else {
        return "EDIT";
    };
    match mode {
        edtui::EditorMode::Insert => "INSERT",
        edtui::EditorMode::Visual => "VISUAL",
        edtui::EditorMode::Search => "SEARCH",
        edtui::EditorMode::Normal => "NORMAL",
    }
}

/// Footer hint for the detail pane. Normally just `Back: ESC`. When a leave was
/// attempted while the draft is dirty (blocked by the save/discard gate — Esc,
/// a focus move, or closing the detail), it shows the interception reason
/// instead; the caller renders it in the same failure colour the connections
/// pane uses for its dirty-leave notice.
fn detail_footer(detail: &DetailState) -> (String, bool) {
    if detail.leave_warning && detail.dirty {
        (
            super::super::detail_edit::DETAIL_LEAVE_WARNING.to_string(),
            true,
        )
    } else {
        ("Back: ESC".to_string(), false)
    }
}

/// The two detail action chips, padded exactly like the list toolbar buttons
/// (`" [C-s] Save "` / `" [C-u] Discard "`), so the Save/Discard row reads the
/// same as the Results action bar.
const SAVE_CHIP: &str = " [C-s] Save ";
const DISCARD_CHIP: &str = " [C-u] Discard ";
/// Gap (in columns) between two chips on the same line.
const CHIP_GAP: usize = 1;

/// Greedily wrap the Save/Discard chips into lines at most `width` columns
/// wide; each chip fits whole on one line and chips that no longer fit start a
/// new line. Returns the wrapped lines, whose count is the *dynamic* height the
/// action row needs (so a narrow detail never clips the buttons). `chip_style`
/// is the per-chip chrome (list toolbar buttons use a selection background).
fn wrap_action_chips(width: usize, chip_style: Style) -> Vec<Line<'static>> {
    let labels = [SAVE_CHIP, DISCARD_CHIP];
    let mut out = Vec::new();
    let mut line = Line::default();
    let mut used = 0usize;
    for label in labels.iter() {
        let w = label.chars().count();
        if used > 0 && used + CHIP_GAP + w > width {
            out.push(std::mem::take(&mut line));
            used = 0;
        }
        if used > 0 {
            line.push_span(Span::raw(" "));
            used += CHIP_GAP;
        }
        line.push_span(Span::styled(*label, chip_style));
        used += w;
    }
    if !line.spans.is_empty() {
        out.push(line);
    }
    out
}

/// Render the detail sub-pane with its own border, optional action buttons,
/// body area, and a detail footer.
///
/// `edit_active` is passed from the parent (list state) since edit mode is
/// owned by the list sub-feature. The returned hit region is the focused
/// editor's text area (pointer → draft gestures).
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
) -> Option<crate::common::editor::EditorMouseHitArea> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let p = theme.palette();

    // The detail is its own bordered panel. Its border doubles as the focus
    // cue: bright while the cell editor holds focus, dim otherwise. While
    // focused the title also carries the current editor mode so Insert /
    // Normal / Visual can be told apart at a glance.
    let title_text = if detail.focused {
        format!("{title}  [{}]", editor_mode_label(detail))
    } else {
        title
    };
    let border_style = if detail.focused {
        p.child_border(focused)
    } else {
        Style::default().fg(p.border)
    };
    let title_style = Style::default().fg(if detail.focused { p.accent } else { p.muted });
    let block = Block::default()
        .title(Line::from(Span::styled(title_text, title_style)))
        .borders(Borders::ALL)
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let has_action_btns = edit_active && detail.dirty && detail.focused;
    let (hint, hint_warn) = detail_footer(detail);
    let footer_h = footer_height(&hint, inner.width).min(3);

    // Reserve a *dynamic* number of rows for the Save/Discard chips: they wrap
    // to as many lines as the detail width needs, so they are never clipped.
    let chip_style = Style::default().bg(p.selection);
    let action_lines = if has_action_btns {
        wrap_action_chips(inner.width as usize, chip_style)
    } else {
        Vec::new()
    };
    let action_h = action_lines.len().max(1) as u16;

    let (body_area, footer_area, action_area) = if has_action_btns {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(action_h),
                Constraint::Min(0),
                Constraint::Length(footer_h),
            ])
            .split(inner);
        (chunks[1], chunks[2], Some(chunks[0]))
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(footer_h)])
            .split(inner);
        (chunks[0], chunks[1], None)
    };

    // Detail action chips (only while editing, focused and dirty), wrapped and
    // drawn with the list toolbar's button chrome.
    if let Some(area) = action_area {
        frame.render_widget(Paragraph::new(action_lines), area);
    }

    // Detail body: the embedded cell editor when focused, otherwise a read-only
    // wrapped preview of the cell value with line numbers. The focused branch
    // hands back the rendered hit region so pointer clicks map onto the draft.
    let mouse_hit = if detail.focused
        && let Some(host) = detail.editor.as_ref()
    {
        let mut editor = host.editor.clone();
        crate::common::editor::render_detail_editor(&mut editor, body_area, frame.buffer_mut())
    } else {
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
        None
    };

    // Detail footer. Normally the plain "Back: ESC"; while an unsaved draft has
    // blocked a leave attempt the interception reason is shown in the failure
    // colour the connections pane uses for its dirty-leave notice.
    if hint_warn {
        let style = Style::default().fg(Color::Red);
        let lines: Vec<Line> = hint
            .split('\n')
            .map(|l| Line::from(Span::styled(l.to_string(), style)))
            .collect();
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }),
            footer_area,
        );
    } else {
        draw_footer(frame, theme, footer_area, &hint);
    }
    mouse_hit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chip_style() -> Style {
        Style::default().bg(Color::White)
    }

    fn line_text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>()
    }

    #[test]
    fn detail_chip_labels_use_the_list_shortcuts() {
        assert_eq!(SAVE_CHIP.trim(), "[C-s] Save");
        assert_eq!(DISCARD_CHIP.trim(), "[C-u] Discard");
    }

    #[test]
    fn chips_stay_on_one_line_when_wide_enough() {
        let lines = wrap_action_chips(200, chip_style());
        assert_eq!(lines.len(), 1, "wide detail keeps both chips on one line");
        let text = line_text(&lines[0]);
        assert!(text.contains("[C-s] Save"));
        assert!(text.contains("[C-u] Discard"));
    }

    #[test]
    fn chips_wrap_when_the_detail_is_narrow() {
        // Wide enough for the Save chip only → Discard wraps to its own line,
        // so the action row needs a dynamic two-row height.
        let lines = wrap_action_chips(SAVE_CHIP.chars().count(), chip_style());
        assert_eq!(lines.len(), 2, "narrow detail must wrap the chips");
        assert!(line_text(&lines[0]).contains("[C-s] Save"));
        assert!(line_text(&lines[1]).contains("[C-u] Discard"));
    }

    #[test]
    fn footer_is_back_esc_normally_and_reason_when_leave_blocked() {
        let mut d = DetailState::default();
        let (text, warn) = detail_footer(&d);
        assert_eq!(text, "Back: ESC", "normal footer keeps only Back: ESC");
        assert!(!warn);

        // A blocked leave (unsaved draft) swaps the footer for the reason.
        d.dirty = true;
        d.leave_warning = true;
        let (text, warn) = detail_footer(&d);
        assert!(warn);
        assert_eq!(
            text,
            crate::features::sql_workspace::sql_tab::results::detail_edit::DETAIL_LEAVE_WARNING
        );

        // Once saved/discarded the flag is cleared again → plain footer.
        d.dirty = false;
        let (text, warn) = detail_footer(&d);
        assert_eq!(text, "Back: ESC");
        assert!(!warn);
    }
}
