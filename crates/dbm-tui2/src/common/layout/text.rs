//! Text metrics: how many terminal rows a piece of text needs.

use crate::common::utils::text_width;

/// Number of terminal rows a hint line occupies when wrapped to `cols` columns.
/// CJK-aware, so wide footers reserve the right height instead of clipping.
pub fn footer_height(text: &str, cols: u16) -> u16 {
    text_width::wrapped_line_count(text, cols)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_height_wraps_by_width() {
        // Single short line fits in one row regardless of a wide column.
        assert_eq!(footer_height("a: x", 100), 1);
        // A long line wraps: 20 chars across a 10-col window is 2 rows.
        assert_eq!(footer_height("a: x  b: y  c: z", 10), 2);
        // An empty text still reserves a single row.
        assert_eq!(footer_height("", 50), 1);
    }
}
