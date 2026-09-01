//! Pure in-buffer search for the SQL editor (`/` query, `n`/`N` next/prev).
//!
//! This module holds the side-effect-free part of editor search: finding the
//! character-column spans of every match across the editor's lines, plus the
//! highlight styles. It operates on plain `&[String]` lines so it never depends
//! on `edtui` or any feature — the editor integration turns the buffer into
//! lines, calls `find_matches`, and applies the results.

use crate::common::components::search::{find_match_starts, TextSearchOptions};

/// A single search match: which line and the inclusive char-column span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqlSearchMatch {
    pub row: usize,
    /// Character column where the match starts (inclusive).
    pub col_start: usize,
    /// Character column where the match ends (exclusive).
    pub col_end: usize,
}

/// Find every non-overlapping match of `query` across `lines` (one `String` per
/// logical editor row), returning char-column spans.
pub fn find_matches(lines: &[String], query: &str, opts: TextSearchOptions) -> Vec<SqlSearchMatch> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let query_len = query.chars().count();
    let mut matches = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        for start in find_match_starts(line, query, opts) {
            matches.push(SqlSearchMatch {
                row,
                col_start: start,
                col_end: start.saturating_add(query_len),
            });
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(case_insensitive: bool) -> TextSearchOptions {
        TextSearchOptions { ignore_case: case_insensitive }
    }

    #[test]
    fn find_matches_respects_case() {
        let lines = vec!["SELECT beta".to_string()];
        assert_eq!(find_matches(&lines, "bet", opts(false)).len(), 1);
        assert!(find_matches(&lines, "BET", opts(false)).is_empty());
        assert_eq!(find_matches(&lines, "BET", opts(true)).len(), 1);
    }

    #[test]
    fn find_matches_handles_unicode_line() {
        let lines = vec!["FROM 测试表".to_string()];
        let hits = find_matches(&lines, "测试", opts(false));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].col_start, 5);
        assert_eq!(hits[0].col_end, 7);
    }

    #[test]
    fn find_matches_empty_query_returns_none() {
        let lines = vec!["SELECT 1".to_string(), "SELECT 2".to_string()];
        assert!(find_matches(&lines, "   ", opts(false)).is_empty());
    }

    #[test]
    fn find_matches_across_multiple_lines() {
        let lines = vec![
            "select id".to_string(),
            "from users".to_string(),
            "where id = 1".to_string(),
        ];
        let hits = find_matches(&lines, "id", opts(false));
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].row, 0);
        assert_eq!(hits[0].col_start, 7);
        assert_eq!(hits[1].row, 2);
        assert_eq!(hits[1].col_start, 6);
    }
}
