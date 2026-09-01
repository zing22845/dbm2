//! In-buffer search for the SQL editor (`/` query, `n`/`N` next/prev match).
//!
//! Owns the search input (`PaneSearch`), the cached matches, and the current
//! match index. The pure matching itself lives in
//! `crate::common::utils::sql_search`; this module adapts it to the editor's
//! `edtui::EditorState` (cursor positioning + highlights).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Style;

use crate::common::components::search::{PaneSearch, PaneSearchInput};
use crate::common::utils::sql_search::{
    SqlSearchMatch, current_match_style, find_matches, other_match_style,
};

/// Search state for one editor: the `/` input plus its live match results.
#[derive(Debug, Clone, Default)]
pub struct EditorSqlSearch {
    pub search: PaneSearch,
    pub matches: Vec<SqlSearchMatch>,
    pub match_index: usize,
}

impl EditorSqlSearch {
    pub fn has_filter(&self) -> bool {
        self.search.has_filter()
    }

    pub fn text_input_active(&self) -> bool {
        self.search.text_input_active()
    }

    pub fn start(&mut self, editor: &mut edtui::EditorState) {
        self.search.start();
        self.refresh(editor);
    }

    pub fn clear(&mut self, editor: &mut edtui::EditorState) {
        self.search.reset();
        self.matches.clear();
        self.match_index = 0;
        editor.clear_highlights();
    }

    /// Recompute matches from the editor buffer and re-apply the current match.
    pub fn refresh(&mut self, editor: &mut edtui::EditorState) {
        let lines: Vec<String> = editor
            .lines
            .iter_row()
            .map(|line| line.iter().collect())
            .collect();
        self.matches = find_matches(&lines, &self.search.query, self.search.options);
        if self.matches.is_empty() {
            self.match_index = 0;
            editor.clear_highlights();
            return;
        }
        if self.match_index >= self.matches.len() {
            self.match_index = 0;
        }
        self.apply_match(editor, self.match_index);
    }

    /// Advance to the next/prev match (cycling), re-applying it.
    pub fn advance(&mut self, editor: &mut edtui::EditorState, forward: bool) {
        if self.search.query.trim().is_empty() || self.matches.is_empty() {
            return;
        }
        let len = self.matches.len();
        self.match_index = if forward {
            (self.match_index + 1) % len
        } else {
            (self.match_index + len - 1) % len
        };
        self.apply_match(editor, self.match_index);
    }

    /// Handle a key while the search input is active. Returns true when the key
    /// was consumed (so the caller knows not to forward it to the editor).
    pub fn handle_search_input(&mut self, editor: &mut edtui::EditorState, key: KeyEvent) -> bool {
        let action = self.search.handle_key(&key, false);
        match action {
            PaneSearchInput::Ignored | PaneSearchInput::Cancelled => {}
            PaneSearchInput::Applied => {
                if !self.matches.is_empty() {
                    self.apply_match(editor, self.match_index);
                }
            }
            PaneSearchInput::Navigate { forward } => self.advance(editor, forward),
            PaneSearchInput::QueryChanged | PaneSearchInput::OptionsChanged => {
                self.refresh(editor);
                if !self.matches.is_empty() {
                    self.match_index = 0;
                    self.apply_match(editor, 0);
                }
            }
        }
        !matches!(action, PaneSearchInput::Ignored)
    }

    fn apply_match(&mut self, editor: &mut edtui::EditorState, index: usize) {
        let Some(m) = self.matches.get(index).copied() else {
            return;
        };
        self.match_index = index;
        editor.cursor = edtui::Index2::new(m.row, m.col_start);
        self.sync_highlights(editor);
    }

    fn sync_highlights(&self, editor: &mut edtui::EditorState) {
        if self.matches.is_empty() {
            editor.clear_highlights();
            return;
        }
        let current = current_match_style();
        let other = other_match_style();
        let highlights = self
            .matches
            .iter()
            .enumerate()
            .map(|(idx, m)| {
                let style: Style = if idx == self.match_index { current } else { other };
                edtui::Highlight::new(
                    edtui::Index2::new(m.row, m.col_start),
                    edtui::Index2::new(m.row, m.col_end.saturating_sub(1)),
                    style,
                )
            })
            .collect();
        editor.set_highlights(highlights);
    }
}

fn search_jump_modifiers_ok(key: &KeyEvent) -> bool {
    key.modifiers.is_empty()
        || key.modifiers.intersects(KeyModifiers::CONTROL)
        || key.modifiers.intersects(KeyModifiers::SHIFT)
}

/// Handle a key at the editor level for `/` search. Returns true when consumed.
///
/// Call this before forwarding a key to `edtui`. When the search input is
/// active, keys go here; otherwise `/` (Normal mode) starts search and `n`/`N`
/// jump while a filter is applied.
pub fn handle_sql_pane_search_key(
    search: &mut EditorSqlSearch,
    editor: &mut edtui::EditorState,
    key: KeyEvent,
) -> bool {
    if search.text_input_active() {
        return search.handle_search_input(editor, key);
    }

    // The case toggle (`Ctrl+/`) keeps working while a filter is applied
    // (input ended via Enter): the query is still shown, so re-run the match
    // with the new case setting instead of letting the key reach the buffer.
    if search.search.is_visible()
        && crate::common::components::search::is_case_toggle_key(&key)
    {
        return search.handle_search_input(editor, key);
    }

    match key.code {
        KeyCode::Char('/')
            if key.modifiers.is_empty() && editor.mode == edtui::EditorMode::Normal =>
        {
            search.start(editor);
            true
        }
        KeyCode::Char(c)
            if c.eq_ignore_ascii_case(&'n')
                && search_jump_modifiers_ok(&key)
                && search.has_filter() =>
        {
            // `N` (Shift) or `/N` goes backwards.
            let forward = !(key.modifiers.contains(KeyModifiers::SHIFT) || c.is_uppercase());
            search.advance(editor, forward);
            true
        }
        KeyCode::Esc if key.modifiers.is_empty() => match search.search.escape() {
            crate::common::components::search::PaneSearchEscape::ClearedFilter => {
                search.matches.clear();
                search.match_index = 0;
                editor.clear_highlights();
                true
            }
            crate::common::components::search::PaneSearchEscape::EndedInput => true,
            crate::common::components::search::PaneSearchEscape::Ignored => false,
        },
        _ => false,
    }
}

/// Case-sensitivity options label helper reused by the editor title.
pub fn case_suffix(search: &PaneSearch) -> String {
    search.case_suffix()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::components::search::TextSearchOptions;
    use crate::common::editor;

    fn with_text(text: &str) -> edtui::EditorState {
        let mut e = editor::new_editor(text);
        e.mode = edtui::EditorMode::Normal;
        e.cursor = edtui::Index2::new(0, 0);
        e
    }

    #[test]
    fn slash_starts_search_in_normal_mode() {
        let mut editor = with_text("select id from users");
        let mut search = EditorSqlSearch::default();
        let key = KeyEvent::new(KeyCode::Char('/'), KeyModifiers::empty());
        assert!(handle_sql_pane_search_key(&mut search, &mut editor, key));
        assert!(search.text_input_active());
    }

    #[test]
    fn query_refresh_finds_and_applies_first_match() {
        let mut editor = with_text("select id from users where id = 1");
        let mut search = EditorSqlSearch::default();
        search.start(&mut editor);
        // Type "id" char by char through the active input.
        for c in ['i', 'd'] {
            let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty());
            search.handle_search_input(&mut editor, key);
        }
        assert_eq!(search.matches.len(), 2);
        assert_eq!(search.matches[0].row, 0);
        assert_eq!(search.matches[0].col_start, 7);
        assert_eq!(editor.cursor.col, 7);
    }

    #[test]
    fn n_advances_to_next_match_and_wraps() {
        let mut editor = with_text("id x id");
        let mut search = EditorSqlSearch::default();
        // Simulate an applied filter.
        search.search.query = "id".into();
        search.search.options = TextSearchOptions::default();
        search.refresh(&mut editor);
        assert_eq!(search.matches.len(), 2);
        assert_eq!(search.match_index, 0);
        let key = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::empty());
        assert!(handle_sql_pane_search_key(&mut search, &mut editor, key));
        assert_eq!(search.match_index, 1);
        // Wrap back to 0.
        assert!(handle_sql_pane_search_key(&mut search, &mut editor, key));
        assert_eq!(search.match_index, 0);
    }

    #[test]
    fn escape_clears_filter_and_highlights() {
        let mut editor = with_text("id x id");
        let mut search = EditorSqlSearch::default();
        search.search.query = "id".into();
        search.search.options = TextSearchOptions::default();
        search.refresh(&mut editor);
        assert!(!search.matches.is_empty());
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::empty());
        assert!(handle_sql_pane_search_key(&mut search, &mut editor, key));
        assert!(search.matches.is_empty());
        assert!(!search.has_filter());
    }
}
