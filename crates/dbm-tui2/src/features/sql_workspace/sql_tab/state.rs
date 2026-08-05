//! `sql_tab` feature state: a collection of tabs, each with its own session
//! and the three child module states (`editor`, `results`, `history`).

use super::session::TabSession;
use super::editor::state::EditorState;
use super::history::state::HistoryState;
use super::results::state::ResultsState;

/// A single SQL tab: an independent session plus the three child module states.
#[derive(Debug, Clone)]
pub struct SqlTab {
    /// This tab's own session (connection, database/schema, persistence unit).
    pub session: TabSession,
    /// Editor child feature state.
    pub editor: EditorState,
    /// Results child feature state.
    pub results: ResultsState,
    /// History child feature state.
    pub history: HistoryState,
}

/// State for the `sql_tab` parent feature: multiple tabs, one active.
#[derive(Debug, Clone)]
pub struct SqlTabState {
    /// All open tabs. Tab indices are targets for routed messages; the stable
    /// identity of a tab lives in its `session.id`.
    pub tabs: Vec<SqlTab>,
    /// Index into `tabs` of the currently active tab.
    pub active_tab: usize,
    /// Monotonic counter for allocating stable session ids to new tabs.
    next_tab_id: usize,
}

impl Default for SqlTabState {
    fn default() -> Self {
        // The application starts with a single empty SQL tab.
        let mut state = SqlTabState {
            tabs: Vec::new(),
            active_tab: 0,
            next_tab_id: 0,
        };
        state.open_tab();
        state
    }
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
