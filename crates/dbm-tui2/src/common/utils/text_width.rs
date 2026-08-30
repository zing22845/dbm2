//! Centralized display-width helpers (CJK / wide-character aware).
//!
//! Any place that turns text into terminal cells must measure with these
//! helpers so manual layout and wrapping agree with what `ratatui` renders.
//! `ratatui` measures text with the same `unicode-width` crate, keeping our
//! computations consistent with the on-screen result.
//!
//! Never use `str::len()` (bytes) or `str::chars().count()` (code points) as a
//! proxy for display width: a CJK char is 3 bytes but 2 cells, and a combining
//! mark is 1 code point but 0 cells.

use unicode_width::UnicodeWidthChar;

/// Display width of the first `char_offset` chars of `text` (CJK-aware).
pub fn prefix_width(text: &str, char_offset: usize) -> usize {
    text.chars().take(char_offset).map(char_width).sum()
}

/// Display width of a single char in terminal cells.
///
/// Control / zero-width combining marks would otherwise read as 0; we floor
/// unknowns at 1 so every cell is at least one column wide.
pub fn char_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0).max(1)
}

/// Display width of `s` in terminal cells (CJK = 2, combining marks = 0).
pub fn width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// Truncate `s` to fit `max_width` display cells, appending `…` when cut.
/// Returns an empty string for a zero-width budget.
pub fn truncate(s: &str, max_width: usize) -> String {
    truncate_from(s, 0, max_width)
}

/// Skip the leading `skip` display cells of `s`, then truncate to `max_width`
/// cells (appending `…` when cut mid-text; not appended when the visible window
/// reaches the true end of `s`). Used for horizontally scrollable panes.
pub fn truncate_from(s: &str, skip: usize, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if skip >= width(s) {
        return String::new();
    }

    // First pass: collect chars starting after `skip`, tracking cell width.
    let chars: Vec<char> = s.chars().collect();
    let widths: Vec<usize> = chars.iter().map(|c| char_width(*c)).collect();

    let mut start_idx = 0usize;
    let mut acc = 0usize;
    while start_idx < chars.len() && acc < skip {
        acc += widths[start_idx];
        if acc > skip {
            // Straddles the skip boundary; keep the overflow tail of this char.
            break;
        }
        start_idx += 1;
    }

    let mut out: Vec<char> = Vec::new();
    let mut used = 0usize;
    let mut idx = start_idx;
    while idx < chars.len() {
        let cw = widths[idx];
        if used + cw > max_width {
            // Truncated mid-text: reserve the last cell for the ellipsis.
            if !out.is_empty() {
                out.pop();
            }
            out.push('…');
            return out.into_iter().collect();
        }
        out.push(chars[idx]);
        used += cw;
        idx += 1;
    }
    out.into_iter().collect()
}

/// Plain (no-ellipsis) version of [`truncate_from`]: skips leading display
/// cells and cuts at `max_width` without appending `…`. Used by panes whose
/// horizontal scrollbar already signals overflow (e.g. explorer object/tree
/// panes) — they don't need the extra visual marker on each row.
pub fn truncate_plain_from(s: &str, skip: usize, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if skip >= width(s) {
        return String::new();
    }

    let chars: Vec<char> = s.chars().collect();
    let widths: Vec<usize> = chars.iter().map(|c| char_width(*c)).collect();

    let mut start_idx = 0usize;
    let mut acc = 0usize;
    while start_idx < chars.len() && acc < skip {
        acc += widths[start_idx];
        if acc > skip {
            break;
        }
        start_idx += 1;
    }

    let mut out: Vec<char> = Vec::new();
    let mut used = 0usize;
    let mut idx = start_idx;
    while idx < chars.len() {
        let cw = widths[idx];
        if used + cw > max_width {
            break;
        }
        out.push(chars[idx]);
        used += cw;
        idx += 1;
    }
    out.into_iter().collect()
}

/// Number of wrapped lines `text` occupies when constrained to `cols` cells.
///
/// Measured with the renderer's own wrapper (`Paragraph` + `Wrap { trim: false }`)
/// rather than estimated from the display width. The renderer wraps at *word*
/// boundaries, which needs **more** rows than a `width / cols` split whenever a
/// word does not fit in the space left on a line — e.g. a 61-cell footer at 30
/// columns renders as 3 rows, not `ceil(61/30) == 2`.
///
/// Getting this right matters because panes reserve `footer_height` rows for
/// their hint: under-counting clips the tail of the footer (silently losing a
/// key hint such as "Collapse: h"), while over-counting merely wastes a row.
///
/// # Why the `unstable-rendered-line-info` feature
///
/// `Paragraph::line_count` sits behind ratatui's `unstable-rendered-line-info`
/// feature (enabled in `dbm-tui2/Cargo.toml`). It is still unstable in 0.30.2 —
/// the latest release — and its stabilization is tracked by ratatui#293
/// ("RFC: Text Wrapping Design"), which is closed but still labelled
/// "Design Needed" with no milestone, so upgrading ratatui will not make it
/// stable any time soon.
///
/// The alternative — rendering into a scratch `Buffer` and counting the rows that
/// received content — needs no unstable feature and was measured to agree with
/// `line_count` at every width tested. It is the fallback if a future ratatui
/// renames or removes this method. Because the only failure mode is a compile
/// error (never a silent behaviour change), and the call is confined to this one
/// function, depending on the unstable API is the better trade: it is exact and
/// allocates nothing.
pub fn wrapped_line_count(text: &str, cols: u16) -> u16 {
    if text.is_empty() {
        return 1;
    }
    use ratatui::widgets::{Paragraph, Wrap};
    let rows = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .line_count(cols.max(1));
    rows.max(1) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_width_equals_len() {
        assert_eq!(width("hello"), 5);
    }

    #[test]
    fn cjk_counts_double_width() {
        assert_eq!(width("科学研究"), 8);
    }

    #[test]
    fn wrapped_ascii_matches_byte_estimate() {
        assert_eq!(wrapped_line_count("abcdefgh", 4), 2);
    }

    #[test]
    fn wrapped_cjk_uses_display_width() {
        // 4 CJK chars = 8 cells; at width 4 that is 2 lines.
        assert_eq!(wrapped_line_count("科学研究", 4), 2);
        // 6 CJK chars = 12 cells; at width 4 that is 3 lines.
        assert_eq!(wrapped_line_count("科学研究管理", 4), 3);
    }

    #[test]
    fn empty_text_is_one_line() {
        assert_eq!(wrapped_line_count("", 4), 1);
    }

    /// Render `text` with ratatui and count the rows that actually received
    /// content — the ground truth `wrapped_line_count` must match, or a pane's
    /// footer gets clipped.
    fn rendered_rows(text: &str, cols: u16) -> usize {
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;
        use ratatui::widgets::{Paragraph, Widget, Wrap};

        let area = Rect::new(0, 0, cols, 16);
        let mut buf = Buffer::empty(area);
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .render(area, &mut buf);
        let mut rows = 0usize;
        for y in 0..area.height {
            let occupied = (0..area.width).any(|x| buf[(x, y)].symbol() != " ");
            if occupied {
                rows = y as usize + 1;
            }
        }
        rows
    }

    #[test]
    fn wrapped_count_matches_rendered_rows_for_a_real_footer() {
        // Regression: a pane reserves `footer_height` rows for its hint. A
        // `width / cols` estimate under-counts because the renderer wraps at
        // word boundaries, clipping the tail of the footer (losing "Collapse: h").
        let text = "Open: ENTER / Dbl-click  Add conn: a  Expand: l  Collapse: h";
        for cols in [60u16, 45, 40, 35, 30, 25, 20, 15, 12] {
            assert_eq!(
                wrapped_line_count(text, cols) as usize,
                rendered_rows(text, cols),
                "wrapped_line_count must match the rendered rows at {cols} columns"
            );
        }
    }

    #[test]
    fn wrapped_count_matches_rendered_rows_for_cjk() {
        // CJK footers must reserve the right height too (display width, not
        // char count).
        let text = "科学研究管理数据";
        for cols in [20u16, 12, 9, 7, 5, 3, 2] {
            assert_eq!(
                wrapped_line_count(text, cols) as usize,
                rendered_rows(text, cols),
                "CJK wrapped_line_count must match the rendered rows at {cols} columns"
            );
        }
    }
}
