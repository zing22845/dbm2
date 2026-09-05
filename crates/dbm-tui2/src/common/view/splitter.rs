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

/// The old (pre-Route-B) splitter stored a `u8` ratio clamped to `20..=80`, so
/// neither pane could drop below 20% of the track height (top ≤ 80% ⇒ bottom ≥
/// 20%). We keep that behaviour by flooring both panes as a percentage of the
/// track.
pub const MIN_SPLIT_PANE_PCT: u8 = 20;

/// Minimum rows for `pct`% of `track_h` — the old ratio floor, expressed in
/// rows (e.g. 20% of a 40-row track = 8 rows).
pub fn min_rows_for_pct(track_h: u16, pct: u8) -> u16 {
    ((u32::from(track_h) * u32::from(pct)) / 100) as u16
}

/// Clamp a horizontal splitter's **top-pane height** (in rows) so neither pane
/// collapses. Both floors are expressed as a percentage of the track height
/// (`min_top_pct`/`min_bottom_pct`), mirroring the old `20..=80` ratio clamp:
/// the top pane stays in `[min_top_pct%, 100 - min_bottom_pct%]` and the bottom
/// pane (splitter row + itself) stays in `[min_bottom_pct%, 100 - min_top_pct%]`.
/// The `Length(1)` splitter row always reserves one row.
pub fn clamp_split_px(px: u16, track_h: u16, min_top_pct: u8, min_bottom_pct: u8) -> u16 {
    let min_top = min_rows_for_pct(track_h, min_top_pct);
    let min_bottom = min_rows_for_pct(track_h, min_bottom_pct);
    // Space that must stay below the top pane: the splitter row + bottom pane.
    let reserved = 1 + min_bottom;
    let max = track_h.saturating_sub(reserved);
    if min_top > max {
        // Track too short to honour both minima; keep the bottom reservation
        // rather than panicking on an inverted clamp range.
        return max;
    }
    px.clamp(min_top, max)
}

/// Keyboard nudge for a **horizontal** splitter, in rows.
///
/// `px` is the current top-pane height. `grow` means "make the focused pane
/// taller"; when the bottom pane is focused the sign flips so `+` still grows
/// the focused side. `step` rows per press. Returns the new height, clamped to
/// `[min_top, max_top]`. A zero effective move returns `px` unchanged, so
/// callers can skip the no-op bump.
pub fn nudge_px_for_focus(
    px: u16,
    grow: bool,
    focus_is_top: bool,
    min_top: u16,
    max_top: u16,
    step: u16,
) -> u16 {
    let sign: i32 = if grow == focus_is_top { 1 } else { -1 };
    let next = px as i32 + sign * step as i32;
    next.clamp(min_top as i32, max_top as i32) as u16
}

/// Keyboard nudge for a **vertical** splitter: `[` → left, `]` → right.
///
/// Call sites never invent sign flips per pane. They only declare whether the
/// width they store is the **left** or **right** side of that splitter; the
/// delta comes from [`width_delta_for_left_pane`] / [`width_delta_for_right_pane`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalSplitterNudge {
    Left,
    Right,
}

/// Default column step for `[` / `]` width nudges.
pub const WIDTH_NUDGE_STEP: i16 = 2;

/// Delta for a width owned by the pane to the **left** of the splitter.
/// Growing that width moves the splitter right.
pub fn width_delta_for_left_pane(nudge: VerticalSplitterNudge, step: i16) -> i16 {
    match nudge {
        VerticalSplitterNudge::Right => step,
        VerticalSplitterNudge::Left => -step,
    }
}

/// Delta for a width owned by the pane to the **right** of the splitter.
/// Growing that width moves the splitter left.
pub fn width_delta_for_right_pane(nudge: VerticalSplitterNudge, step: i16) -> i16 {
    -width_delta_for_left_pane(nudge, step)
}

pub fn hit(rect: Rect, x: u16, y: u16) -> bool {
    rect.width > 0
        && rect.height > 0
        && x >= rect.x
        && x < rect.right()
        && y >= rect.y
        && y < rect.bottom()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_split_px_reserves_splitter_and_both_panes() {
        // Both floors 20%. 20% of 40 = 8; reserved below = 1 (splitter) + 8 = 9.
        // top in [8, 40-9=31]; bottom in [8, 32].
        assert_eq!(clamp_split_px(100, 40, 20, 20), 31); // above max -> 31
        // below top min (8) -> clamped up to 8 (bottom then = 40-1-8 = 31)
        assert_eq!(clamp_split_px(1, 40, 20, 20), 8);
        // above max -> clamped down to max so bottom keeps 20%
        assert_eq!(clamp_split_px(100, 10, 20, 20), 7); // max = 10-1-2 = 7
        // tiny track: 20% of 3 = 0 for both floors -> top capped at max=2
        assert_eq!(clamp_split_px(10, 3, 20, 20), 2);
        assert_eq!(clamp_split_px(10, 0, 20, 20), 0);
    }

    #[test]
    fn min_rows_for_pct_matches_old_ratio_floor() {
        assert_eq!(min_rows_for_pct(40, 20), 8);
        assert_eq!(min_rows_for_pct(20, 20), 4);
        assert_eq!(min_rows_for_pct(3, 20), 0);
    }

    #[test]
    fn nudge_px_for_focus_grows_focused_side() {
        // top focused, grow -> top taller
        assert_eq!(nudge_px_for_focus(40, true, true, 0, 80, 3), 43);
        // top focused, shrink -> top shorter
        assert_eq!(nudge_px_for_focus(40, false, true, 0, 80, 3), 37);
        // bottom focused, grow -> top shorter
        assert_eq!(nudge_px_for_focus(40, true, false, 0, 80, 3), 37);
        // bottom focused, shrink -> top taller
        assert_eq!(nudge_px_for_focus(40, false, false, 0, 80, 3), 43);
    }

    #[test]
    fn nudge_px_for_focus_clamps() {
        assert_eq!(nudge_px_for_focus(79, true, true, 0, 80, 3), 80);
        assert_eq!(nudge_px_for_focus(1, false, true, 0, 80, 3), 0);
        // zero effective move returns unchanged
        assert_eq!(nudge_px_for_focus(80, true, true, 0, 80, 3), 80);
    }

    #[test]
    fn hit_requires_nonzero_area() {
        assert!(!hit(Rect::default(), 0, 0));
        let r = Rect::new(2, 3, 4, 1);
        assert!(hit(r, 2, 3));
        assert!(!hit(r, 6, 3));
    }

    #[test]
    fn vertical_splitter_bracket_deltas_by_side() {
        // Left-owned width (Explorer tree, History Detail): ] grows, [ shrinks.
        assert_eq!(
            width_delta_for_left_pane(VerticalSplitterNudge::Right, WIDTH_NUDGE_STEP),
            WIDTH_NUDGE_STEP
        );
        assert_eq!(
            width_delta_for_left_pane(VerticalSplitterNudge::Left, WIDTH_NUDGE_STEP),
            -WIDTH_NUDGE_STEP
        );
        // Right-owned width (History pane from SQL, Results Detail): ] shrinks, [ grows.
        assert_eq!(
            width_delta_for_right_pane(VerticalSplitterNudge::Right, WIDTH_NUDGE_STEP),
            -WIDTH_NUDGE_STEP
        );
        assert_eq!(
            width_delta_for_right_pane(VerticalSplitterNudge::Left, WIDTH_NUDGE_STEP),
            WIDTH_NUDGE_STEP
        );
    }
}
