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
use ratatui::style::Style;
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

/// Which detail action chip (Save / Discard) was clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailChip {
    Save,
    Discard,
}

impl DetailChip {
    pub fn label(self) -> &'static str {
        match self {
            DetailChip::Save => SAVE_CHIP,
            DetailChip::Discard => DISCARD_CHIP,
        }
    }
}

/// A placed Save/Discard chip: its row and column offset inside the detail
/// pane's inner area (border already subtracted).
#[derive(Debug, Clone, Copy)]
pub struct DetailChipPlace {
    pub chip: DetailChip,
    pub row: u16,
    pub x: u16,
    pub width: u16,
}

/// Greedily place the Save/Discard chips into rows of at most `width` columns;
/// each chip fits whole on one row and chips that no longer fit start a new
/// row. The same placements drive rendering and the pointer hit-test, so a
/// click always hits the painted chip.
pub fn place_detail_chips(width: usize) -> Vec<DetailChipPlace> {
    let labels = [
        (DetailChip::Save, SAVE_CHIP),
        (DetailChip::Discard, DISCARD_CHIP),
    ];
    let mut out = Vec::with_capacity(labels.len());
    let mut row = 0u16;
    let mut used = 0usize;
    for (chip, label) in labels {
        let w = label.chars().count();
        if used > 0 && used + CHIP_GAP + w > width.max(1) {
            row = row.saturating_add(1);
            used = 0;
        }
        let x = if used > 0 { used + CHIP_GAP } else { used };
        out.push(DetailChipPlace {
            chip,
            row,
            x: x as u16,
            width: w as u16,
        });
        used = x + w;
    }
    out
}

/// The number of rows the placed chips occupy (at least 1).
pub fn detail_chip_rows(width: usize) -> u16 {
    place_detail_chips(width)
        .iter()
        .map(|p| p.row)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

/// Hit-test `(x, y)` against the visible Save/Discard chips. The chips are only
/// shown (and therefore clickable) while the draft is dirty, focused and inside
/// an edit session; `chip_visible` is exactly that condition.
pub fn detail_chip_hit(
    detail_area: Rect,
    chip_visible: bool,
    x: u16,
    y: u16,
) -> Option<DetailChip> {
    if !chip_visible || detail_area.width < 3 || detail_area.height < 2 {
        return None;
    }
    let inner_x = detail_area.x.saturating_add(1);
    let inner_top = detail_area.y.saturating_add(1);
    let places = place_detail_chips(detail_area.width.saturating_sub(2) as usize);
    for place in places {
        let rect = Rect {
            x: inner_x.saturating_add(place.x),
            y: inner_top.saturating_add(place.row),
            width: place.width,
            height: 1,
        };
        if x >= rect.x && x < rect.x.saturating_add(rect.width) && y == rect.y {
            return Some(place.chip);
        }
    }
    None
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
    // to as many rows as the detail width needs, so they are never clipped.
    let chip_style = Style::default().bg(p.selection);
    let action_h = if has_action_btns {
        detail_chip_rows(inner.width as usize)
    } else {
        1
    };

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

    // Detail action chips (only while editing, focused and dirty), placed and
    // drawn with the list toolbar's button chrome.
    if let Some(area) = action_area {
        for place in place_detail_chips(area.width as usize) {
            if place.row >= action_h || place.width == 0 {
                continue;
            }
            let left = area.x.saturating_add(place.x);
            let width = (area.right() - left).min(place.width);
            if width == 0 {
                continue;
            }
            let visible: String = place.chip.label().chars().take(width as usize).collect();
            frame.render_widget(
                Paragraph::new(visible).style(chip_style),
                Rect {
                    x: left,
                    y: area.y.saturating_add(place.row),
                    width,
                    height: 1,
                },
            );
        }
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
    // blocked a leave attempt the interception reason is shown in the theme's
    // warning colour.
    if hint_warn {
        let style = Style::default().fg(p.warning);
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

    #[test]
    fn detail_chip_labels_use_the_list_shortcuts() {
        assert_eq!(SAVE_CHIP.trim(), "[C-s] Save");
        assert_eq!(DISCARD_CHIP.trim(), "[C-u] Discard");
    }

    #[test]
    fn chips_stay_on_one_row_when_wide_enough() {
        let places = place_detail_chips(200);
        assert_eq!(places.len(), 2, "both chips are always placed");
        assert_eq!(detail_chip_rows(200), 1, "wide detail keeps one row");
        assert!(places.iter().all(|p| p.row == 0));
    }

    #[test]
    fn chips_wrap_when_the_detail_is_narrow() {
        // Wide enough for the Save chip only → Discard wraps onto its own row.
        let width = SAVE_CHIP.chars().count();
        let places = place_detail_chips(width);
        assert_eq!(detail_chip_rows(width), 2, "narrow detail must wrap");
        assert_eq!(places[0].row, 0);
        assert_eq!(places[0].chip, DetailChip::Save);
        assert_eq!(places[1].row, 1);
        assert_eq!(places[1].chip, DetailChip::Discard);
    }

    #[test]
    fn chip_hit_tests_the_visible_chip_rects() {
        // Wide pane: both chips on the single action row (area includes the
        // 1-col borders, so chips start at x+1/y+1).
        let area = Rect::new(0, 0, 60, 3);
        assert_eq!(
            detail_chip_hit(area, true, 2, 1),
            Some(DetailChip::Save),
            "the first chip's text area maps to Save"
        );
        let discard_x = 1 + place_detail_chips(58)[1].x;
        assert_eq!(
            detail_chip_hit(area, true, discard_x + 1, 1),
            Some(DetailChip::Discard)
        );
        // The chips are only hit while visible.
        assert_eq!(detail_chip_hit(area, false, 2, 1), None);
        assert_eq!(detail_chip_hit(area, true, 2, 2), None, "outside chip row");
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
