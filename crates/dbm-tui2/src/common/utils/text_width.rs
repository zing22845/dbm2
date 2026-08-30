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

/// Estimated number of wrapped lines for `text` constrained to `cols` cells.
///
/// Display-width aware, so CJK footers reserve the right height. For pure
/// ASCII this matches the previous `line.len().div_ceil(width)` estimate.
pub fn wrapped_line_count(text: &str, cols: u16) -> u16 {
    if text.is_empty() {
        return 1;
    }
    let w = cols.max(1) as usize;
    text.lines()
        .map(|line| {
            let line_w = width(line);
            if line_w == 0 {
                1
            } else {
                line_w.div_ceil(w)
            }
        })
        .sum::<usize>()
        .max(1) as u16
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
}
