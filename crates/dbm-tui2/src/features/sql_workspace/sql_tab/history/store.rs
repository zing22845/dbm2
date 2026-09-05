//! SQL history storage (per-connection, newest-first, deduplicated) and the
//! line / scroll helpers the history list and detail panes share.

use std::collections::HashMap;

use dbm_store::SQL_HISTORY_MAX_PER_CONNECTION;
use unicode_width::UnicodeWidthStr;

use crate::common::components::search::PaneSearch;
use crate::common::utils::text_width;

pub fn history_limit_hint() -> String {
    format!("max {SQL_HISTORY_MAX_PER_CONNECTION} · newest first")
}

pub fn history_empty_hint() -> String {
    format!("No history yet · max {SQL_HISTORY_MAX_PER_CONNECTION}")
}

pub fn history_no_matches_hint() -> &'static str {
    "No matching history"
}

/// In-memory SQL history keyed by `(instance, connection)`, newest first.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SqlHistoryStore {
    by_connection: HashMap<(String, String), Vec<String>>,
}

impl SqlHistoryStore {
    pub fn from_map(map: HashMap<(String, String), Vec<String>>) -> Self {
        Self { by_connection: map }
    }

    /// Record a successful statement: dedupe, move to front, cap at the limit.
    pub fn record_success(&mut self, instance: &str, connection: &str, sql: &str) {
        let trimmed = sql.trim();
        if trimmed.is_empty() {
            return;
        }
        let key = (instance.to_string(), connection.to_string());
        let list = self.by_connection.entry(key).or_default();
        if let Some(pos) = list.iter().position(|s| s == trimmed) {
            list.remove(pos);
        }
        list.insert(0, trimmed.to_string());
        list.truncate(SQL_HISTORY_MAX_PER_CONNECTION);
    }

    pub fn entries(&self, instance: &str, connection: &str) -> &[String] {
        self.by_connection
            .get(&(instance.to_string(), connection.to_string()))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn get(&self, instance: &str, connection: &str, index: usize) -> Option<&str> {
        self.entries(instance, connection)
            .get(index)
            .map(String::as_str)
    }
}

/// First (non-empty) line of a history statement, trimmed.
pub fn history_one_line(sql: &str) -> &str {
    sql.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(sql)
        .trim()
}

/// Horizontal display: skip `skip` cells, then truncate to `width` cells.
pub fn history_display_line(sql: &str, skip: u16, width: u16) -> String {
    text_width::truncate_from(history_one_line(sql), skip as usize, width as usize)
}

pub fn history_max_h_scroll(sql: &str, text_viewport: u16) -> u16 {
    history_line_display_width(sql).saturating_sub(text_viewport)
}

pub fn history_line_display_width(sql: &str) -> u16 {
    UnicodeWidthStr::width(history_one_line(sql)) as u16
}

pub fn history_max_v_scroll(row_count: usize, viewport_rows: usize) -> usize {
    row_count.saturating_sub(viewport_rows.max(1))
}

pub fn clamp_history_v_scroll(offset: usize, row_count: usize, viewport_rows: usize) -> usize {
    offset.min(history_max_v_scroll(row_count, viewport_rows))
}

/// Indices into `entries` matching the search query (all when no filter).
pub fn history_visible_indices(entries: &[String], search: &PaneSearch) -> Vec<usize> {
    search.matching_indices(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::components::search::TextSearchOptions;

    #[test]
    fn newest_first_and_dedupes() {
        let mut store = SqlHistoryStore::default();
        store.record_success("pg", "default", "SELECT 1");
        store.record_success("pg", "default", "SELECT 2");
        store.record_success("pg", "default", "SELECT 1");
        let entries = store.entries("pg", "default");
        assert_eq!(entries, &["SELECT 1", "SELECT 2"]);
    }

    #[test]
    fn horizontal_skip_shows_suffix_at_end() {
        let text = "SELECT * FROM very_long_table_name";
        let width = 12u16;
        let max_skip = history_max_h_scroll(text, width);
        let rendered = history_display_line(text, max_skip, width);
        assert!(!rendered.ends_with('…'));
        assert_eq!(unicode_width::UnicodeWidthStr::width(rendered.as_str()), 12);
    }

    #[test]
    fn truncates_display_label() {
        let label = history_display_line("SELECT * FROM very_long_table_name", 0, 12);
        assert!(label.ends_with('…'));
        assert!(label.chars().count() <= 12);
    }

    #[test]
    fn matching_indices_preserves_entry_positions() {
        let entries = vec![
            "SELECT 1".into(),
            "SELECT users".into(),
            "INSERT INTO t".into(),
        ];
        let search = PaneSearch {
            query: "SELECT".into(),
            ..PaneSearch::default()
        };
        let hits = search.matching_indices(&entries);
        assert_eq!(hits, vec![0, 1]);
        assert_eq!(hits.iter().position(|&i| i == 1), Some(1));
    }

    #[test]
    fn matching_indices_respects_ignore_case_option() {
        let entries = vec!["SELECT 1".into(), "INSERT INTO t".into()];
        let search = PaneSearch {
            query: "select".into(),
            options: TextSearchOptions { ignore_case: true },
            ..PaneSearch::default()
        };
        assert_eq!(search.matching_indices(&entries), vec![0]);
    }
}
