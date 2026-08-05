//! Identifier parsing helpers (pure, byte-offset based).
//!
//! These utilities operate on byte offsets into a UTF-8 buffer and are used by
//! the completion engine's context analysis.

/// Snap a byte offset down to the nearest valid UTF-8 char boundary.
pub fn snap_to_char_boundary(s: &str, mut pos: usize) -> usize {
    pos = pos.min(s.len());
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// Advance `pos` by one full UTF-8 character (or to `s.len()`).
pub fn advance_byte_offset(s: &str, pos: usize) -> usize {
    if pos >= s.len() {
        return s.len();
    }
    let pos = snap_to_char_boundary(s, pos);
    if pos >= s.len() {
        return s.len();
    }
    pos + s[pos..].chars().next().map(char::len_utf8).unwrap_or(0)
}

/// Safe prefix slice using a byte end offset.
pub fn byte_prefix(s: &str, end: usize) -> &str {
    &s[..snap_to_char_boundary(s, end)]
}

/// Byte offset where the current SQL statement starts (after previous `;` / `；`).
pub fn statement_start_before(sql: &str, offset: usize) -> usize {
    let offset = snap_to_char_boundary(sql, offset.min(sql.len()));
    let slice = &sql[..offset];
    let mut start = 0usize;
    for (idx, ch) in slice.char_indices() {
        if ch == ';' || ch == '；' {
            start = idx + ch.len_utf8();
        }
    }
    start
}

pub fn unquote_ident(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(inner) = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        inner.replace("\"\"", "\"")
    } else {
        trimmed.to_string()
    }
}

pub fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || matches!(c, '_' | '@' | '$' | '#')
}

pub fn is_ident_part(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '@' | '$' | '#')
}

pub fn prev_char_start(s: &str, pos: usize) -> usize {
    let mut pos = pos.min(s.len());
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    if pos == 0 {
        return 0;
    }
    s[..pos].char_indices().last().map(|(i, _)| i).unwrap_or(0)
}

pub fn scan_ident_end(input: &str) -> usize {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        return 0;
    }
    let skip = input.len() - trimmed.len();
    if trimmed.starts_with('"') {
        let mut end = 1usize;
        let chars: Vec<char> = trimmed.chars().collect();
        while end < chars.len() {
            if chars[end] == '"' {
                if chars.get(end + 1) == Some(&'"') {
                    end += 2;
                    continue;
                }
                end += 1;
                break;
            }
            end += 1;
        }
        let byte_len: usize = chars[..end].iter().map(|c| c.len_utf8()).sum();
        return skip + byte_len;
    }
    let mut byte_len = 0usize;
    for ch in trimmed.chars() {
        if !(is_ident_part(ch) || ch == '.') {
            break;
        }
        byte_len += ch.len_utf8();
    }
    skip + byte_len
}

pub fn scan_trailing_ident_range(before: &str) -> (usize, usize, bool) {
    let trimmed = before.trim_end();
    if trimmed.is_empty() {
        return (before.len(), before.len(), false);
    }
    let ends_with_dot = trimmed.ends_with('.');
    let mut end = trimmed.len();
    if ends_with_dot {
        end -= '.'.len_utf8();
    }
    let mut start = end;
    while start > 0 {
        let ch_start = prev_char_start(trimmed, start);
        let ch = trimmed[ch_start..].chars().next().unwrap_or('\0');
        if ch == '"' {
            if let Some(quote_start) = trimmed[..ch_start].rfind('"') {
                start = quote_start;
            }
            break;
        }
        if is_ident_part(ch) || ch == '.' {
            start = ch_start;
            continue;
        }
        break;
    }
    let scan_end = if ends_with_dot {
        trimmed.len() - '.'.len_utf8()
    } else {
        trimmed.len()
    };
    (start, scan_end, ends_with_dot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_recovers_from_mid_utf8() {
        let s = "测";
        assert_eq!(snap_to_char_boundary(s, 1), 0);
        assert_eq!(snap_to_char_boundary(s, 3), 3);
    }

    #[test]
    fn advance_skips_full_utf8_char() {
        let s = "x测y";
        assert_eq!(advance_byte_offset(s, 0), 1);
        assert_eq!(advance_byte_offset(s, 1), 4);
        assert_eq!(advance_byte_offset(s, 2), 4);
        assert_eq!(advance_byte_offset(s, 4), 5);
    }

    #[test]
    fn statement_start_handles_fullwidth_semicolon() {
        let sql = "select 1； select 2";
        let offset = sql.len();
        assert_eq!(statement_start_before(sql, offset), "select 1；".len());
    }

    #[test]
    fn unquote_unicode_identifier() {
        assert_eq!(unquote_ident("\"测试\""), "测试");
    }
}
