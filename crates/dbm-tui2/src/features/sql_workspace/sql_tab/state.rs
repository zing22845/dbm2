//! `sql_tab` feature state: a collection of tabs, each with its own session
//! and the three child module states (`editor`, `results`, `history`).

use super::session::TabSession;
use super::editor::state::EditorState;
use super::history::state::HistoryState;
use super::results::state::ResultsState;

/// Which sub-pane of the SQL tab currently owns the keyboard focus. The editor
/// and results/history panes share the workspace, so keys must be routed to one
/// of them based on this focus (mirrors the original `SqlFocusPane`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SqlFocus {
    /// The SQL editor (default).
    #[default]
    Editor,
    /// The results grid / edit session.
    Results,
    /// The history list.
    History,
}

/// Default editor top-pane height (percent of the body) for the SQL tab's
/// horizontal splitter.
pub const DEFAULT_SPLIT_RATIO: u8 = 45;
/// Default history pane width (columns) for the SQL tab's vertical splitter.
pub const DEFAULT_HISTORY_WIDTH: u16 = 24;
/// Min/max editor top-pane height as a percent of the body.
pub const MIN_SPLIT_RATIO: u8 = 20;
pub const MAX_SPLIT_RATIO: u8 = 80;
/// Min/max history pane width in columns.
pub const MIN_HISTORY_WIDTH: u16 = 16;
pub const MAX_HISTORY_WIDTH: u16 = 200;

/// A single SQL tab: an independent session plus the three child module states.
#[derive(Debug, Clone)]
pub struct SqlTab {
    /// This tab's own session (connection, database/schema, persistence unit).
    pub session: TabSession,
    /// The sub-pane currently focused (routes keys within this tab).
    pub focus: SqlFocus,
    /// Editor top-pane height as a percent of the body (horizontal splitter).
    pub split_ratio: u8,
    /// History pane width in columns (vertical splitter between editor/history).
    pub history_pane_width: u16,
    /// Editor child feature state.
    pub editor: EditorState,
    /// Results child feature state.
    pub results: ResultsState,
    /// History child feature state.
    pub history: HistoryState,
}

impl SqlTab {
    /// Clamp and store the editor top-pane height percentage.
    pub fn set_split_ratio(&mut self, ratio: u8) {
        self.split_ratio = ratio.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO);
    }

    /// Clamp and store the history pane width (columns).
    pub fn set_history_pane_width(&mut self, width: u16) {
        self.history_pane_width = width.clamp(MIN_HISTORY_WIDTH, MAX_HISTORY_WIDTH);
    }
}

/// State for the `sql_tab` parent feature: multiple tabs, one active.
///
/// `Default` starts with **no** tabs: a query tab is only opened when the user
/// selects a connection in the explorer, matching the original dbm (which
/// starts with `tabs: Vec::new()` and shows an empty-state hint until then).
#[derive(Debug, Clone, Default)]
pub struct SqlTabState {
    /// All open tabs. Tab indices are targets for routed messages; the stable
    /// identity of a tab lives in its `session.id`.
    pub tabs: Vec<SqlTab>,
    /// Index into `tabs` of the currently active tab.
    pub active_tab: usize,
    /// Monotonic counter for allocating stable session ids to new tabs.
    next_tab_id: usize,
}

impl SqlTabState {
    /// Open a fresh tab (new session + empty child states) and make it active.
    pub fn open_tab(&mut self) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(SqlTab {
            session: TabSession {
                id,
                ..TabSession::default()
            },
            focus: SqlFocus::default(),
            split_ratio: DEFAULT_SPLIT_RATIO,
            history_pane_width: DEFAULT_HISTORY_WIDTH,
            editor: EditorState::default(),
            results: ResultsState::default(),
            history: HistoryState::default(),
        });
        self.active_tab = self.tabs.len() - 1;
    }

    /// Open a new tab bound to a connection, carrying its display identity so
    /// history keys and query execution use real names.
    pub fn open_connection_tab(
        &mut self,
        instance: String,
        connection: String,
        connection_id: String,
        database: Option<String>,
        schema: Option<String>,
    ) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(SqlTab {
            session: TabSession {
                id,
                connection_id: Some(connection_id),
                instance: Some(instance),
                connection: Some(connection),
                database,
                schema,
            },
            focus: SqlFocus::default(),
            split_ratio: DEFAULT_SPLIT_RATIO,
            history_pane_width: DEFAULT_HISTORY_WIDTH,
            editor: EditorState::default(),
            results: ResultsState::default(),
            history: HistoryState::default(),
        });
        self.active_tab = self.tabs.len() - 1;
    }

    /// Close the tab at `idx`. The active index is repaired; closing the last
    /// remaining tab leaves an empty tab list.
    pub fn close_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        self.tabs.remove(idx);
        if self.tabs.is_empty() {
            self.active_tab = 0;
        } else {
            self.active_tab = self.active_tab.min(self.tabs.len() - 1);
        }
    }

    /// Return the index of the tab whose stable `session.id` equals `tab_id`,
    /// or `None` if no such tab exists.
    pub fn index_of(&self, tab_id: usize) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.session.id == tab_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_with_no_tabs() {
        let state = SqlTabState::default();
        assert!(state.tabs.is_empty(), "no tab should be auto-created");
    }

    #[test]
    fn open_connection_tab_binds_session_identity() {
        let mut state = SqlTabState::default();
        state.open_connection_tab(
            "local".into(),
            "app-db".into(),
            "conn-42".into(),
            Some("mydb".into()),
            Some("public".into()),
        );
        let session = &state.tabs[state.active_tab].session;
        assert_eq!(session.instance.as_deref(), Some("local"));
        assert_eq!(session.connection.as_deref(), Some("app-db"));
        assert_eq!(session.connection_id.as_deref(), Some("conn-42"));
        assert_eq!(session.database.as_deref(), Some("mydb"));
        assert_eq!(session.schema.as_deref(), Some("public"));
    }

    #[test]
    fn index_of_finds_tab_by_session_id() {
        let mut state = SqlTabState::default();
        state.open_tab();
        let id = state.tabs[state.active_tab].session.id;
        assert_eq!(state.index_of(id), Some(state.active_tab));
        assert_eq!(state.index_of(999_999), None);
    }
}

impl SqlTabState {
    /// Stable session id of the active tab (test helper).
    #[allow(dead_code)]
    fn active_tab_id(&self) -> usize {
        self.tabs
            .get(self.active_tab)
            .map(|t| t.session.id)
            .unwrap_or(0)
    }
}
