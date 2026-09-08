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
use crate::common::view::pane_scrollbar::draw_vertical_pane_scrollbar;
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

/// Footer hint for the detail pane. The normal `Back: ESC` line always stays
/// first; when a leave was attempted while the draft is dirty (blocked by the
/// save/discard gate — Esc, a focus move, or closing the detail) the
/// interception reason is returned too, and the caller renders it BELOW the
/// normal footer in the failure colour the connections pane uses.
fn detail_footer(detail: &DetailState) -> (&'static str, Option<&'static str>) {
    let warning = if detail.leave_warning && detail.dirty {
        Some(super::super::detail_edit::DETAIL_LEAVE_WARNING)
    } else {
        None
    };
    ("Back: ESC", warning)
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

/// The detail body's 1-column vertical scrollbar track along its right edge.
/// Only meaningful while the content overflows the body height (the caller
/// decides); the bar occupies the column the body content is narrowed by.
fn detail_v_scrollbar_rect(body: Rect) -> Option<Rect> {
    if body.width == 0 || body.height == 0 {
        return None;
    }
    Some(Rect {
        x: body.right().saturating_sub(1),
        y: body.y,
        width: 1,
        height: body.height,
    })
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
    let (hint, leave_warn) = detail_footer(detail);
    // The footer keeps the normal hint and, when a leave was blocked by an
    // unsaved draft, appends the interception reason below it (extra rows are
    // reserved so the body shrinks instead of the two overlapping).
    let hint_h = footer_height(hint, inner.width);
    let warn_h = leave_warn.map_or(0, |w| footer_height(w, inner.width));
    let footer_h = (hint_h + warn_h).min(inner.height.saturating_sub(2));

    // Reserve a *dynamic* number of rows for the Save/Discard chips: they wrap
    // to as many rows as the detail width needs, so they are never clipped.
    // The chips reuse the list toolbar's available-button chrome (the header
    // Discover look) so an enabled action reads the same everywhere.
    let chip_style = p.available_button_style();
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
    //
    // Both modes can overflow the pane height, so the body owns a vertical
    // scrollbar of its own: a right-hand track column is reserved (and drawn)
    // only when the content is taller than the body. Like the SQL editor, the
    // bar decision is two-pass — first guess the scrollbar to learn the real
    // wrap width, then recount rows at the narrowed width so the bar's max
    // matches what the renderer actually produced.
    let viewport_rows = body_area.height.max(1) as usize;
    let bar = detail_v_scrollbar_rect(body_area);
    let mouse_hit = if detail.focused
        && let Some(host) = detail.editor.as_ref()
    {
        let gutter_w = crate::common::editor::editor_line_number_gutter_width(&host.editor);
        // Pass 1: would the wrapped text overflow at the full width?
        let probe_wrap = body_area.width.saturating_sub(gutter_w).max(1);
        let probe_rows = crate::common::editor::editor_display_row_count(&host.editor, probe_wrap);
        let needs_v = probe_rows > viewport_rows;
        let content_width = body_area.width.saturating_sub(u16::from(needs_v)).max(1);
        let wrap_width = content_width.saturating_sub(gutter_w).max(1);
        let row_count = crate::common::editor::editor_display_row_count(&host.editor, wrap_width);
        let content = Rect {
            x: body_area.x,
            y: body_area.y,
            width: content_width,
            height: body_area.height,
        };

        let mut editor = host.editor.clone();
        let hit =
            crate::common::editor::render_detail_editor(&mut editor, content, frame.buffer_mut());
        // Draw the scrollbar after the editor: it reports the viewport edtui
        // actually rendered with (the value the run loop also syncs back).
        if needs_v
            && row_count > viewport_rows
            && let Some(bar) = bar
        {
            let max_scroll = row_count.saturating_sub(viewport_rows);
            let scroll = crate::common::editor::editor_v_scroll_display(&editor, wrap_width);
            draw_vertical_pane_scrollbar(
                frame,
                bar,
                scroll.min(max_scroll),
                viewport_rows,
                max_scroll,
                p,
                false,
            );
        }
        hit
    } else {
        // Pass 1: does the wrapped preview overflow at the full body width?
        let probe_total = detail_display_line_count(body, body_area.width);
        let needs_v = probe_total > viewport_rows;
        let content_width = body_area.width.saturating_sub(u16::from(needs_v)).max(1);
        let display_lines = build_detail_lines(body, content_width);
        let lines_total = display_lines.len();
        let mut detail_state = detail.clone();
        detail_state.clamp_scroll(lines_total, viewport_rows);
        let visible: Vec<Line> = display_lines
            .into_iter()
            .skip(detail_state.scroll)
            .take(viewport_rows.max(1))
            .collect();
        let content = Rect {
            x: body_area.x,
            y: body_area.y,
            width: content_width,
            height: body_area.height,
        };
        frame.render_widget(Paragraph::new(visible), content);
        if needs_v
            && lines_total > viewport_rows
            && let Some(bar) = bar
        {
            let max_scroll = lines_total.saturating_sub(viewport_rows);
            draw_vertical_pane_scrollbar(
                frame,
                bar,
                detail_state.scroll.min(max_scroll),
                viewport_rows,
                max_scroll,
                p,
                false,
            );
        }
        None
    };

    // Detail footer. The normal "Back: ESC" hint keeps its rows on top; while
    // an unsaved draft has blocked a leave attempt the interception reason is
    // appended below it in the theme's warning colour.
    let normal_area = Rect {
        x: footer_area.x,
        y: footer_area.y,
        width: footer_area.width,
        height: hint_h.min(footer_area.height),
    };
    draw_footer(frame, theme, normal_area, hint);
    if let Some(warn_text) = leave_warn
        && hint_h < footer_area.height
    {
        let style = Style::default().fg(p.warning);
        let lines: Vec<Line> = warn_text
            .split('\n')
            .map(|l| Line::from(Span::styled(l.to_string(), style)))
            .collect();
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }),
            Rect {
                x: footer_area.x,
                y: footer_area.y.saturating_add(hint_h),
                width: footer_area.width,
                height: footer_area.height.saturating_sub(hint_h),
            },
        );
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
    fn footer_keeps_back_esc_and_appends_the_reason_when_leave_blocked() {
        let mut d = DetailState::default();
        let (text, warn) = detail_footer(&d);
        assert_eq!(text, "Back: ESC");
        assert!(warn.is_none(), "clean detail has no leave warning");

        // A blocked leave (unsaved draft) keeps "Back: ESC" and appends the
        // interception reason below it — the normal footer is never replaced.
        d.dirty = true;
        d.leave_warning = true;
        let (text, warn) = detail_footer(&d);
        assert_eq!(text, "Back: ESC", "the normal hint must survive");
        assert_eq!(
            warn,
            Some(
                crate::features::sql_workspace::sql_tab::results::detail_edit::DETAIL_LEAVE_WARNING
            )
        );

        // Once saved/discarded the flag is cleared again → plain footer.
        d.dirty = false;
        let (text, warn) = detail_footer(&d);
        assert_eq!(text, "Back: ESC");
        assert!(warn.is_none());
    }

    #[test]
    fn detail_v_scrollbar_rect_spans_the_right_edge() {
        let body = Rect::new(5, 3, 20, 9);
        let bar = detail_v_scrollbar_rect(body).expect("a non-empty body has a track");
        assert_eq!(bar.width, 1);
        assert_eq!(bar.height, body.height);
        assert_eq!(bar.x, body.right() - 1);
        assert_eq!(bar.y, body.y);
        assert_eq!(detail_v_scrollbar_rect(Rect::new(0, 0, 0, 9)), None);
    }

    #[test]
    fn overflowing_preview_and_editor_draw_a_vertical_scrollbar() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let theme = crate::common::view::theme::default();
        let area = Rect::new(0, 0, 40, 16);
        let body: String = (0..40)
            .map(|i| format!("row {i:02} of a tall cell value"))
            .collect::<Vec<_>>()
            .join("\n");
        let bar_col = area.right().saturating_sub(2); // inner right edge (bar col)

        // Read-only preview overflowing the body height shows the track.
        let detail = DetailState::default();
        let mut t1 = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        t1.draw(|f| {
            let _ = render(
                f,
                &theme,
                area,
                &detail,
                body.as_str(),
                " [id] row 1 ".to_string(),
                false,
                true,
            );
        })
        .unwrap();
        let buf1 = t1.backend().buffer().clone();
        assert!(
            (2..area.height.saturating_sub(2)).any(|y| {
                buf1.cell((bar_col, y))
                    .is_some_and(|c| c.symbol() == "┊" || c.symbol() == "█")
            }),
            "read-only preview overflow must draw its vertical scrollbar"
        );

        // Focused cell editor overflowing the body height also draws it.
        let mut focused = DetailState::default();
        focused.focus_editor(body.as_str());
        let mut t2 = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        t2.draw(|f| {
            let _ = render(
                f,
                &theme,
                area,
                &focused,
                body.as_str(),
                " [id] row 1 ".to_string(),
                false,
                true,
            );
        })
        .unwrap();
        let buf2 = t2.backend().buffer().clone();
        assert!(
            (2..area.height.saturating_sub(2)).any(|y| {
                buf2.cell((bar_col, y))
                    .is_some_and(|c| c.symbol() == "┊" || c.symbol() == "█")
            }),
            "focused detail editor overflow must draw its vertical scrollbar"
        );
    }
}
