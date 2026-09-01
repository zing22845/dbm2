//! Unified in-pane `/` search: state, keyboard input, matching, and title suffix.
//!
//! A self-contained TEA-neutral component shared by panes that support
//! filtering by typing a query (`PaneSearch`). It keeps no app or feature
//! dependencies: callers feed it key events and read back a `PaneSearchInput`,
//! and render a title suffix through [`pane_search_title_line`].

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::common::utils::shortcuts::{
    caps_lock_active, effective_ascii_letter, pane_jump_modifiers_ok,
};

/// Text-search options for a pane's `/` filter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct TextSearchOptions {
    pub ignore_case: bool,
}

impl TextSearchOptions {
    pub fn toggle_ignore_case(&mut self) {
        self.ignore_case = !self.ignore_case;
    }

    pub fn case_label(self) -> &'static str {
        if self.ignore_case { "aa" } else { "Aa" }
    }
}

/// Character-index starts of every non-overlapping match of `needle` in `haystack`.
pub fn find_match_starts(haystack: &str, needle: &str, opts: TextSearchOptions) -> Vec<usize> {
    let needle = needle.trim();
    if needle.is_empty() {
        return Vec::new();
    }

    let mut starts = Vec::new();
    if opts.ignore_case {
        let hay_lower = haystack.to_lowercase();
        let needle_lower = needle.to_lowercase();
        let mut search_from = 0usize;
        while let Some(rel) = hay_lower[search_from..].find(&needle_lower) {
            let byte_start = search_from + rel;
            starts.push(haystack[..byte_start].chars().count());
            search_from = byte_start + needle_lower.len().max(1);
        }
    } else {
        let mut search_from = 0usize;
        while let Some(rel) = haystack[search_from..].find(needle) {
            let byte_start = search_from + rel;
            starts.push(haystack[..byte_start].chars().count());
            search_from = byte_start + needle.len().max(1);
        }
    }
    starts
}

/// Search input state for one pane column.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct PaneSearch {
    pub active: bool,
    pub query: String,
    pub options: TextSearchOptions,
}

/// The outcome of feeding one key to a `PaneSearch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneSearchInput {
    Ignored,
    Cancelled,
    Applied,
    QueryChanged,
    OptionsChanged,
    /// Readline-style `CTRL+p` / `CTRL+n`: move the target-pane cursor without leaving input mode.
    Navigate { forward: bool },
}

/// Shared `Esc` ladder for every pane that uses [`PaneSearch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneSearchEscape {
    /// Not editing and no applied filter — caller may use `Esc` for something else.
    Ignored,
    /// Left edit mode; query (filter) unchanged.
    EndedInput,
    /// Cleared an applied filter while not editing.
    ClearedFilter,
}

impl PaneSearch {
    pub fn start(&mut self) {
        self.active = true;
    }

    pub fn end(&mut self) {
        self.active = false;
    }

    pub fn clear_query(&mut self) {
        self.query.clear();
    }

    /// Unified search `Esc`: end input first, then clear applied filter.
    pub fn escape(&mut self) -> PaneSearchEscape {
        if self.active {
            self.end();
            PaneSearchEscape::EndedInput
        } else if self.has_filter() {
            self.clear_query();
            PaneSearchEscape::ClearedFilter
        } else {
            PaneSearchEscape::Ignored
        }
    }

    /// Readline-style `CTRL+u`: clear the query (no in-query caret, so clear all).
    pub fn clear_to_line_start(&mut self) {
        self.query.clear();
    }

    pub fn reset(&mut self) {
        self.active = false;
        self.query.clear();
    }

    pub fn text_input_active(&self) -> bool {
        self.active
    }

    pub fn has_filter(&self) -> bool {
        !self.query.trim().is_empty()
    }

    pub fn is_visible(&self) -> bool {
        self.active || self.has_filter()
    }

    pub fn case_suffix(&self) -> String {
        if self.is_visible() {
            format!(" {}", self.options.case_label())
        } else {
            String::new()
        }
    }

    /// Build a title suffix ` /query_cursor Aa` truncated to fit `title_width` chars.
    pub fn truncated_title_suffix(&self, label: &str, title_width: u16) -> String {
        if !self.is_visible() {
            return String::new();
        }
        let (mut text, truncated) =
            self.truncated_query_part(label, title_width, false, self.active);
        if truncated {
            text.push('…');
        }
        if self.active {
            text.push('_');
        }
        text.push_str(&self.case_suffix());
        text
    }

    /// Query text truncated for title bar; reserves trailing ` Aa` when `split_case_label`.
    pub fn truncated_query_part(
        &self,
        label: &str,
        title_width: u16,
        split_case_label: bool,
        reserve_cursor: bool,
    ) -> (String, bool) {
        let case_reserve = if split_case_label {
            1 + self.options.case_label().chars().count()
        } else {
            self.case_suffix().chars().count()
        };
        let cursor_len = if reserve_cursor { 1 } else { 0 };
        let prefix_len = label.chars().count() + 3 + case_reserve;
        let avail = title_width as usize;
        let query_budget = avail.saturating_sub(prefix_len + cursor_len);
        let query_chars: Vec<char> = self.query.chars().collect();
        if query_chars.len() <= query_budget {
            (self.query.clone(), false)
        } else if query_budget == 0 {
            (String::new(), !self.query.is_empty())
        } else {
            (
                query_chars
                    .iter()
                    .take(query_budget.saturating_sub(1))
                    .collect(),
                true,
            )
        }
    }

    pub fn matching_indices(&self, entries: &[String]) -> Vec<usize> {
        let q = self.query.trim();
        if q.is_empty() {
            return (0..entries.len()).collect();
        }
        entries
            .iter()
            .enumerate()
            .filter(|(_, text)| !find_match_starts(text, q, self.options).is_empty())
            .map(|(idx, _)| idx)
            .collect()
    }

    pub fn handle_key(&mut self, key: &KeyEvent, caps_lock: bool) -> PaneSearchInput {
        match key.code {
            KeyCode::Esc => {
                // Editing path only (callers invoke `handle_key` while `active`).
                let _ = self.escape();
                PaneSearchInput::Cancelled
            }
            KeyCode::Enter => {
                self.end();
                PaneSearchInput::Applied
            }
            KeyCode::Backspace => {
                if self.query.is_empty() {
                    // Nothing left to delete (e.g. holding backspace after the
                    // query is already empty): ignore so we don't re-render.
                    PaneSearchInput::Ignored
                } else {
                    self.query.pop();
                    PaneSearchInput::QueryChanged
                }
            }
            _ if is_clear_line_key(key) => {
                if self.query.is_empty() {
                    PaneSearchInput::Ignored
                } else {
                    self.clear_to_line_start();
                    PaneSearchInput::QueryChanged
                }
            }
            _ if is_case_toggle_key(key) => {
                self.options.toggle_ignore_case();
                PaneSearchInput::OptionsChanged
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                PaneSearchInput::Navigate { forward: false }
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                PaneSearchInput::Navigate { forward: true }
            }
            KeyCode::Char(c) if pane_jump_modifiers_ok(key.modifiers) => {
                let c = if c.is_ascii() {
                    effective_ascii_letter(
                        c,
                        key.modifiers.contains(KeyModifiers::SHIFT),
                        caps_lock_active(key, caps_lock),
                    )
                } else {
                    c
                };
                self.query.push(c);
                PaneSearchInput::QueryChanged
            }
            _ => PaneSearchInput::Ignored,
        }
    }
}

/// Toggle ignore-case. Terminals disagree on Ctrl+/: crossterm 0.29's Unix
/// parser maps the raw US `Ctrl+/` byte (0x1F, also `Ctrl+_`) to `Char('7')` +
/// CONTROL, while terminals using the enhanced (kitty) keyboard protocol
/// report `Char('/')`/`Char('_')` + CONTROL. Accept all forms.
pub fn is_case_toggle_key(key: &KeyEvent) -> bool {
    if !key.modifiers.contains(KeyModifiers::CONTROL) {
        return false;
    }
    matches!(
        key.code,
        KeyCode::Char('/')
            | KeyCode::Char('_')
            | KeyCode::Char('\x1f')
            | KeyCode::Char('7')
    )
}

/// Readline `CTRL+u` — some terminals emit `u`, others `\x15`.
fn is_clear_line_key(key: &KeyEvent) -> bool {
    if !key.modifiers.contains(KeyModifiers::CONTROL) {
        return false;
    }
    matches!(key.code, KeyCode::Char('u') | KeyCode::Char('\x15'))
}

pub fn inactive_search_style(theme_muted: Style) -> Style {
    theme_muted
}

/// Style of the active search input: the caller supplies the theme-derived
/// accent style (this component is theme-agnostic).
pub fn search_query_style(search: &PaneSearch, active_style: Style, theme_muted: Style) -> Style {
    if search.active {
        active_style
    } else {
        inactive_search_style(theme_muted)
    }
}

fn case_separator_style(match_style: Style) -> Style {
    let mut style = Style::default();
    if let Some(fg) = match_style.fg {
        style = style.fg(fg);
    }
    if match_style.add_modifier != Modifier::empty() {
        style = style.add_modifier(match_style.add_modifier);
    }
    style
}

/// Helpers take several styles/width knobs to keep the title line flexible;
/// the argument count is a faithful port of the reference search bar.
#[allow(clippy::too_many_arguments)]
fn append_search_query_spans(
    spans: &mut Vec<Span<'static>>,
    search: &PaneSearch,
    active_style: Style,
    applied_match_style: Option<Style>,
    inactive_style: Style,
    title_width: Option<u16>,
    label: &str,
    input_focused: bool,
) {
    spans.push(Span::raw("  /".to_string()));
    if search.active {
        let cursor = if input_focused { "_" } else { "" };
        let text = if let Some(width) = title_width {
            let (query, truncated) =
                search.truncated_query_part(label, width, false, input_focused);
            let mut text = query;
            if truncated {
                text.push('…');
            }
            text.push_str(cursor);
            text.push_str(&search.case_suffix());
            text
        } else {
            format!("{}{}{}", search.query, cursor, search.case_suffix())
        };
        spans.push(Span::styled(text, active_style));
        return;
    }
    if let Some(match_style) = applied_match_style.filter(|_| search.has_filter()) {
        let (query, truncated) = if let Some(width) = title_width {
            search.truncated_query_part(label, width, true, false)
        } else {
            (search.query.clone(), false)
        };
        spans.push(Span::styled(query, match_style));
        if truncated {
            spans.push(Span::styled("…".to_string(), match_style));
        }
        spans.push(Span::styled(
            " ".to_string(),
            case_separator_style(match_style),
        ));
        spans.push(Span::styled(
            search.options.case_label().to_string(),
            match_style,
        ));
        return;
    }
    let text = if let Some(width) = title_width {
        search.truncated_title_suffix(label, width)
    } else {
        format!("{}{}", search.query, search.case_suffix())
    };
    spans.push(Span::styled(text, inactive_style));
}

pub fn filter_nav_counter(search: &PaneSearch, cursor: usize, filtered_count: usize) -> String {
    if search.has_filter() && filtered_count > 0 {
        format!(
            " {}/{}",
            cursor.min(filtered_count.saturating_sub(1)) + 1,
            filtered_count
        )
    } else {
        String::new()
    }
}

/// Label-only title line (no search query or counter). Used when the search
/// query/counter is rendered on the bottom border (`Block::title_bottom`)
/// instead of the top border.
#[allow(clippy::too_many_arguments)]
pub fn pane_search_label_line(
    label: &str,
    pane_focused: bool,
    highlight_label_when_focused: bool,
    theme_muted: Style,
    focused_label_style: Option<Style>,
    accent_style: Style,
) -> Line<'static> {
    let label_style = if highlight_label_when_focused && pane_focused {
        focused_label_style.unwrap_or(accent_style)
    } else if highlight_label_when_focused {
        theme_muted
    } else {
        Style::default()
    };
    Line::from(Span::styled(label.to_string(), label_style))
}

/// Bottom-title line for a pane search: `/query [n/m]`. Rendered on the pane's
/// bottom border (via `Block::title_bottom`), taking over part of the bottom
/// border while the search is visible. Returns `None` when search is not
/// visible (no active input and no filter), so the caller draws a plain border.
///
/// `extra` (when non-empty) is appended as a plain suffix after the counter,
/// used by the results list to show `scope` / `count` / `offset` read-outs.
#[allow(clippy::too_many_arguments)]
pub fn pane_search_bottom_title_line(
    search: &PaneSearch,
    pane_focused: bool,
    cursor: usize,
    filtered_count: usize,
    width: Option<u16>,
    theme_muted: Style,
    extra: Option<&str>,
    applied_match_style: Option<Style>,
    accent_style: Style,
) -> Option<Line<'static>> {
    if !search.is_visible() {
        return None;
    }
    // Reuse the title-line builder with an empty label so only the search
    // query and counter appear on the bottom border. The leading "  /"
    // separator from `append_search_query_spans` acts as left indentation.
    let mut line = pane_search_title_line(
        "",
        search,
        pane_focused,
        false,
        theme_muted,
        cursor,
        filtered_count,
        width,
        None,
        applied_match_style,
        accent_style,
    );
    if let Some(extra) = extra.filter(|e| !e.is_empty()) {
        line.spans.push(Span::raw(extra.to_string()));
    }
    Some(line)
}

/// Title line for a pane/column label plus optional in-title search and filter counter.
#[allow(clippy::too_many_arguments)]
pub fn pane_search_title_line(
    label: &str,
    search: &PaneSearch,
    pane_focused: bool,
    highlight_label_when_focused: bool,
    theme_muted: Style,
    cursor: usize,
    filtered_count: usize,
    title_width: Option<u16>,
    focused_label_style: Option<Style>,
    applied_match_style: Option<Style>,
    accent_style: Style,
) -> Line<'static> {
    let label_style = if highlight_label_when_focused && pane_focused {
        focused_label_style.unwrap_or(accent_style)
    } else if highlight_label_when_focused {
        theme_muted
    } else {
        Style::default()
    };
    if !search.is_visible() {
        return Line::from(Span::styled(label.to_string(), label_style));
    }
    let counter = filter_nav_counter(search, cursor, filtered_count);
    let applied =
        applied_match_style.filter(|_| pane_focused && !search.active && search.has_filter());
    let mut spans = vec![Span::styled(label.to_string(), label_style)];
    append_search_query_spans(
        &mut spans,
        search,
        accent_style,
        applied,
        inactive_search_style(theme_muted),
        title_width,
        label,
        pane_focused,
    );
    spans.push(Span::styled(counter, label_style));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;
    use crossterm::event::{KeyEventKind, KeyEventState};

    #[test]
    fn title_line_includes_slash_prefix_when_search_active() {
        let mut search = PaneSearch::default();
        search.start();
        search.query = "users".into();
        let line = pane_search_title_line(
            " databases ",
            &search,
            true,
            false,
            Style::default(),
            0,
            1,
            Some(40),
            None,
            None,
            Style::default().fg(Color::Blue),
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains(" databases "), "{text}");
        assert!(text.contains("  /users"), "{text}");
    }

    #[test]
    fn case_sensitive_match() {
        let opts = TextSearchOptions::default();
        assert_eq!(find_match_starts("beta", "bet", opts), vec![0]);
        assert!(find_match_starts("beta", "BET", opts).is_empty());
    }

    #[test]
    fn ignore_case_match() {
        let opts = TextSearchOptions { ignore_case: true };
        assert_eq!(find_match_starts("beta", "BET", opts), vec![0]);
    }

    #[test]
    fn matching_indices_respects_options() {
        let entries = vec!["SELECT 1".into(), "INSERT INTO t".into()];
        let mut search = PaneSearch {
            query: "select".into(),
            ..Default::default()
        };
        assert!(search.matching_indices(&entries).is_empty());
        search.options.ignore_case = true;
        assert_eq!(search.matching_indices(&entries), vec![0]);
    }

    #[test]
    fn ctrl_slash_toggles_ignore_case() {
        let mut search = PaneSearch::default();
        search.start();
        let key = KeyEvent {
            code: KeyCode::Char('/'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        assert_eq!(
            search.handle_key(&key, false),
            PaneSearchInput::OptionsChanged
        );
        assert!(search.options.ignore_case);
    }

    #[test]
    fn crossterm_ctrl_slash_char_7_toggles_ignore_case() {
        // crossterm 0.29's Unix parser maps the raw US Ctrl+/ byte (0x1F) to
        // `Char('7')` + CONTROL; the toggle must accept that form too.
        let mut search = PaneSearch::default();
        search.start();
        let key = KeyEvent {
            code: KeyCode::Char('7'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        assert_eq!(
            search.handle_key(&key, false),
            PaneSearchInput::OptionsChanged
        );
        assert!(search.options.ignore_case);
    }

    #[test]
    fn ctrl_u_clears_query() {
        let mut search = PaneSearch::default();
        search.start();
        search.query = "select *".into();
        let key = KeyEvent {
            code: KeyCode::Char('u'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        assert_eq!(
            search.handle_key(&key, false),
            PaneSearchInput::QueryChanged
        );
        assert!(search.query.is_empty());
        assert!(search.active);
    }

    #[test]
    fn escape_ends_input_then_clears_applied_filter() {
        let mut search = PaneSearch::default();
        search.start();
        search.query = "foo".into();
        assert_eq!(search.escape(), PaneSearchEscape::EndedInput);
        assert!(!search.active);
        assert!(search.has_filter());
        assert_eq!(search.escape(), PaneSearchEscape::ClearedFilter);
        assert!(!search.has_filter());
        assert_eq!(search.escape(), PaneSearchEscape::Ignored);
    }

    #[test]
    fn backspace_on_empty_query_is_ignored() {
        let mut search = PaneSearch::default();
        search.start();
        // Delete the only char, then hold backspace again: the second press is
        // ignored so the caller won't re-render on every key repeat.
        let bs = KeyEvent {
            code: KeyCode::Backspace,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        search.query = "a".into();
        assert_eq!(
            search.handle_key(&bs, false),
            PaneSearchInput::QueryChanged
        );
        assert!(search.query.is_empty());
        assert_eq!(search.handle_key(&bs, false), PaneSearchInput::Ignored);
        assert!(search.query.is_empty());
        assert!(search.active);
    }

    #[test]
    fn ctrl_p_n_navigate_without_leaving_input() {
        let mut search = PaneSearch::default();
        search.start();
        search.query = "sel".into();
        let prev = KeyEvent {
            code: KeyCode::Char('p'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let next = KeyEvent {
            code: KeyCode::Char('n'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        assert_eq!(
            search.handle_key(&prev, false),
            PaneSearchInput::Navigate { forward: false }
        );
        assert_eq!(
            search.handle_key(&next, false),
            PaneSearchInput::Navigate { forward: true }
        );
        assert!(search.active);
        assert_eq!(search.query, "sel");
    }
}
