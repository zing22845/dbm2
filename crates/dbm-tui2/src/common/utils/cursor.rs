//! A lightweight, editor-agnostic text cursor.
//!
//! Replaces the legacy `edtui::Index2` dependency so that editor-adjacent
//! features (completion, context analysis) depend on this abstraction rather
//! than on a concrete editor crate (dependency inversion). A cursor is a
//! zero-based `(row, col)` position where `col` counts **characters** (not
//! bytes), so multi-byte UTF-8 (e.g. CJK) is handled consistently.
//!
//! The conversion helpers (`cursor_to_byte_offset` / `byte_offset_to_cursor`)
//! bridge between the char-based cursor and byte offsets used for string
//! slicing.

/// A `(row, col)` position in a text buffer. `col` is in characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
}

impl Cursor {
    pub const fn new(row: usize, col: usize) -> Self {
        Cursor { row, col }
    }
}

/// Byte offset of `cursor` within `text`. `col` is interpreted as a count of
/// characters from the start of `cursor.row`; stops at the end of the buffer.
pub fn cursor_to_byte_offset(text: &str, cursor: Cursor) -> usize {
    let mut row = 0usize;
    let mut col = 0usize;
    let mut byte_offset = 0usize;
    for ch in text.chars() {
        if row == cursor.row && col == cursor.col {
            break;
        }
        byte_offset += ch.len_utf8();
        if ch == '\n' {
            row += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    byte_offset
}

/// The `(row, col)` cursor for a byte offset within `text`. The offset is
/// snapped down to the nearest UTF-8 char boundary first.
pub fn byte_offset_to_cursor(text: &str, mut offset: usize) -> Cursor {
    offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let mut row = 0usize;
    let mut col = 0usize;
    let mut bytes_seen = 0usize;
    for ch in text.chars() {
        if bytes_seen >= offset {
            break;
        }
        bytes_seen += ch.len_utf8();
        if ch == '\n' {
            row += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    Cursor::new(row, col)
}

/// Whether `cursor` is at the end of its line (or the end of the buffer).
pub fn cursor_at_line_end(text: &str, cursor: Cursor) -> bool {
    let offset = cursor_to_byte_offset(text, cursor);
    text[offset..].chars().next().is_none_or(|c| c == '\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_offset_roundtrip_ascii() {
        let text = "select * from users";
        for (idx, _) in text.char_indices() {
            let cur = byte_offset_to_cursor(text, idx);
            assert_eq!(cursor_to_byte_offset(text, cur), idx);
        }
    }

    #[test]
    fn byte_offset_handles_multiline_unicode() {
        let text = "SELECT\n测试表";
        // Row 0 starts at byte 0; row 1 starts after the '\n'.
        let cursor = Cursor::new(1, 0);
        let offset = cursor_to_byte_offset(text, cursor);
        assert_eq!(&text[offset..], "测试表");
        let back = byte_offset_to_cursor(text, offset);
        assert_eq!(back, cursor);
    }

    #[test]
    fn cursor_at_line_end_true_for_eol() {
        let text = "select * from 测试表 w";
        let cursor = Cursor::new(0, text.chars().count());
        assert!(cursor_at_line_end(text, cursor));
    }
}
