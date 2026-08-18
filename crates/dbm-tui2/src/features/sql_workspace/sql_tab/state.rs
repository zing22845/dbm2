//! `sql_tab` feature state: a collection of tabs, each with its own session
//! and the three child module states (`editor`, `results`, `history`).
//!
//! Tabs are stored in a flat `Vec`, but only tabs belonging to the *active
//! connection* are shown (via `visible_tab_indices`). Each connection counts
//! its own tabs independently — `sequence` starts at 1 per connection.

use std::collections::HashMap;

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
/// Maximum number of open tabs per connection (and thus the highest sequence
/// number, `<SQL N>` with `N <= 9`), matching the original dbm's tab limit.
pub const MAX_TABS: usize = 9;
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
    /// The `Editor`/`History` sub-pane that was active before entering Results,
    /// so Ctrl+Up from Results returns to the previous pane (mirroring the
    /// original dbm's `workspace_upper_pane`). Defaults to `Editor`.
    pub upper_pane: SqlFocus,
    /// Editor top-pane height as a percent of the body (horizontal splitter).
    pub split_ratio: u8,
    /// History pane width in columns (vertical splitter between editor/history).
    pub history_pane_width: u16,
    /// Whether table-name completion (TblCmp) is enabled for this tab, shown in
    /// the editor header while in INSERT mode (matching the original dbm's
    /// `complete_table_names`). Toggled with Alt+Tab in INSERT mode.
    pub complete_table_names: bool,
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
    /// All open tabs across all connections. Tab indices are targets for routed
    /// messages; the stable identity of a tab lives in its `session.id`.
    pub tabs: Vec<SqlTab>,
    /// Index into `tabs` of the currently active tab (global index), or `None`
    /// when the active connection has no open query tab (empty workspace).
    pub active_tab: Option<usize>,
    /// Monotonic counter for allocating stable session ids to new tabs.
    next_tab_id: usize,
    /// Per-connection last-active tab global index, so switching connections
    /// restores the previously active tab for that connection.
    connection_last_tab: HashMap<(String, String), usize>,
    /// Which connection's tabs are currently visible in the tab bar.
    pub active_connection: Option<(String, String)>,
}

impl SqlTabState {
    /// Derive the `(instance, connection)` key from session identity fields.
    fn connection_key(&self, session: &TabSession) -> (String, String) {
        let instance = session.instance.clone().unwrap_or_default();
        let connection = session
            .connection
            .clone()
            .or_else(|| session.connection_id.clone())
            .unwrap_or_default();
        (instance, connection)
    }

    /// Return the next per-connection sequence number: one greater than the
    /// largest sequence among the connection's currently-open tabs (or 1 if
    /// none). This matches the original dbm, where numbering is derived from
    /// the live tabs, so after closing `<sql 2>`..`<sql 6>` the next tab is
    /// `<sql 2>` again rather than a monotonic counter continuing at 7.
    fn next_sequence(&self, key: &(String, String)) -> usize {
        self.tabs
            .iter()
            .filter(|tab| self.connection_key(&tab.session) == *key)
            .map(|tab| tab.session.sequence)
            .max()
            .unwrap_or(0)
            + 1
    }

    /// Whether the connection already has `MAX_TABS` open tabs. This is the
    /// single source of the per-connection tab limit (max 9 tabs, so sequence
    /// numbers stay within `<SQL 1>`..`<SQL 9>`), shared by every tab-creation
    /// path (`open_connection_tab`, `open_tab`, and via those the explorer's
    /// Enter / `n` and the workspace's `Alt+t`).
    fn at_tab_limit(&self, key: &(String, String)) -> bool {
        self.tabs
            .iter()
            .filter(|tab| self.connection_key(&tab.session) == *key)
            .count()
            >= MAX_TABS
    }

    /// Returns global indices of tabs whose session matches `active_connection`.
    pub fn visible_tab_indices(&self) -> Vec<usize> {
        let Some(ref conn) = self.active_connection else {
            return Vec::new();
        };
        self.tabs
            .iter()
            .enumerate()
            .filter(|(_i, tab)| self.connection_key(&tab.session) == *conn)
            .map(|(i, _tab)| i)
            .collect()
    }

    /// Number of tabs visible for the active connection.
    pub fn visible_tab_count(&self) -> usize {
        let Some(ref conn) = self.active_connection else {
            return 0;
        };
        self.tabs.iter().filter(|tab| self.connection_key(&tab.session) == *conn).count()
    }

    /// Map a visible-tab offset (0-based, within the active connection's tabs)
    /// to a global `tabs` index. Returns `None` when the offset is out of range
    /// or no active connection is set.
    pub fn visible_to_global(&self, visible_idx: usize) -> Option<usize> {
        let indices = self.visible_tab_indices();
        indices.get(visible_idx).copied()
    }

    /// The tab currently active, or `None` when the active connection has no
    /// open query tab.
    pub fn active_tab(&self) -> Option<&SqlTab> {
        self.active_tab.and_then(|i| self.tabs.get(i))
    }

    /// The currently active connection `(instance, connection)`, if any.
    pub fn active_connection(&self) -> Option<(&str, &str)> {
        self.active_connection
            .as_ref()
            .map(|(i, c)| (i.as_str(), c.as_str()))
    }

    /// Whether the active connection currently has no visible query tab (i.e.
    /// the workspace should show the empty-state hint for it).
    pub fn active_connection_is_empty(&self) -> bool {
        self.visible_tab_indices().is_empty()
    }

    /// Map the current `active_tab` (global) to its visible offset. Returns
    /// `None` when the active tab doesn't belong to the active connection.
    pub fn global_to_visible(&self) -> Option<usize> {
        let indices = self.visible_tab_indices();
        indices.iter().position(|&i| Some(i) == self.active_tab)
    }

    /// Close the context picker on the tab currently active. Called before
    /// switching the active tab / connection so a picker left open on one tab
    /// never keeps blocking another tab's editor (keys or context clicks) after
    /// the user has navigated away.
    pub fn close_active_context_picker(&mut self) {
        if let Some(tab) = self.active_tab.and_then(|i| self.tabs.get_mut(i)) {
            tab.editor.context_picker.close();
        }
    }

    /// Keep the active connection's "current tab" bookmark in sync with the
    /// currently active tab. Called when the active tab changes within a
    /// connection (e.g. `Tab` switching), so a later new tab (`Alt+t`/`n`)
    /// inherits the *currently active* tab's context rather than a stale one.
    pub fn remember_active_tab_for_connection(&mut self) {
        if let Some((key, idx)) = self.active_tab.and_then(|idx| {
            self.tabs
                .get(idx)
                .map(|tab| (self.connection_key(&tab.session), idx))
        }) {
            self.connection_last_tab.insert(key, idx);
        }
    }

    /// Activate a connection: show only that connection's tabs, and restore the
    /// last-active tab for it (or the first tab, `None` when it has none).
    pub fn activate_connection(&mut self, instance: String, connection: String) {
        let key = (instance, connection);
        if self.active_connection == Some(key.clone()) {
            return; // already active, no change
        }
        self.active_connection = Some(key.clone());
        // Restore the last-active tab for this connection, or pick the first
        // visible tab.
        if let Some(&global_idx) = self.connection_last_tab.get(&key) {
            // Verify the stored tab still exists and belongs to this connection.
            if global_idx < self.tabs.len()
                && self.connection_key(&self.tabs[global_idx].session) == key
            {
                self.active_tab = Some(global_idx);
                return;
            }
        }
        // Fall back to the first visible tab (None when this connection has none).
        self.active_tab = self.visible_tab_indices().first().copied();
    }

    /// Update `connection_last_tab` with the current active tab before
    /// deactivating a connection.
    fn save_connection_last_tab(&mut self) {
        let (Some(key), Some(idx)) = (&self.active_connection, self.active_tab) else {
            return;
        };
        if idx < self.tabs.len() {
            self.connection_last_tab.insert(key.clone(), idx);
        }
    }

    /// Open a fresh tab for the currently active connection and make it active.
    /// If no connection is active, this is a no-op (a tab without a connection
    /// would be invisible and unreachable).
    pub fn open_tab(&mut self) {
        let Some(key) = self.active_connection.clone() else {
            return;
        };
        // Enforce the per-connection tab limit (shared `at_tab_limit` check).
        if self.at_tab_limit(&key) {
            return;
        }
        let id = self.next_session_id();
        let sequence = self.next_sequence(&key);
        // A fresh tab inherits the connection's current database/schema context
        // (from its existing tab) instead of starting blank `…/…`. `Alt+t` and
        // instances `n` both go through this, so they share one rule. `Alt+t`
        // is pressed on an existing tab, so `default_database` is never reached.
        let (database, schema) = self.inherit_context_for(&key, None, None, None);
        let (instance, connection) = (Some(key.0.clone()), Some(key.1.clone()));
        self.tabs.push(SqlTab {
            session: TabSession {
                id,
                sequence,
                instance,
                connection,
                database,
                schema,
                ..TabSession::default()
            },
            focus: SqlFocus::default(),
            upper_pane: SqlFocus::Editor,
            split_ratio: DEFAULT_SPLIT_RATIO,
            history_pane_width: DEFAULT_HISTORY_WIDTH,
            complete_table_names: false,
            editor: EditorState::default(),
            results: ResultsState::default(),
            history: HistoryState::default(),
        });
        self.active_tab = Some(self.tabs.len() - 1);
        // Record this tab as the connection's last-active tab so every
        // connection always has a current tab (even one never switched away
        // from), letting new tabs inherit its context reliably.
        self.connection_last_tab.insert(key.clone(), self.tabs.len() - 1);
    }

    /// Resolve the database/schema for a newly opened tab. An explicit
    /// `database`/`schema` (e.g. from the objects tree when opening a table)
    /// wins; otherwise the new tab inherits the *target connection's* context
    /// in this order: (1) its last-active tab (`connection_last_tab`, what
    /// `Enter` restores), (2) its last-created tab (`visible_tab_indices_for`),
    /// and finally (3) `default_database` + `"public"` (mirroring the original
    /// dbm's `connection_entry_database`), so a fresh connection never starts on
    /// the blank `…/…` placeholder.
    fn inherit_context_for(
        &self,
        key: &(String, String),
        database: Option<String>,
        schema: Option<String>,
        default_database: Option<&str>,
    ) -> (Option<String>, Option<String>) {
        if database.is_some() || schema.is_some() {
            return (database, schema);
        }
        // 1) The connection's last-active tab.
        if let Some(&idx) = self
            .connection_last_tab
            .get(key)
            .filter(|&&i| i < self.tabs.len() && self.connection_key(&self.tabs[i].session) == *key)
        {
            return (
                self.tabs[idx].session.database.clone(),
                self.tabs[idx].session.schema.clone(),
            );
        }
        // 2) The connection's last-created tab.
        if let Some(&idx) = self.visible_tab_indices_for(key).last() {
            return (
                self.tabs[idx].session.database.clone(),
                self.tabs[idx].session.schema.clone(),
            );
        }
        // 3) Fall back to the connection's configured default database + "public".
        (
            default_database.map(str::to_string),
            Some("public".to_string()),
        )
    }

    /// Focus an existing tab bound to this connection if one exists (restoring
    /// the last-active tab for it, else the last one), otherwise open a new one.
    /// Mirrors the original dbm's `confirm_workspace_connection(force_new=false)`:
    /// Enter on a connection row switches to that connection's already-open tab
    /// instead of always opening a fresh editor. Returns `true` when a new tab
    /// was created (so the caller can seed its completion catalog), `false`
    /// when an existing tab was focused.
    pub fn focus_or_open_connection_tab(
        &mut self,
        instance: String,
        connection: String,
        connection_id: String,
        database: Option<String>,
        schema: Option<String>,
        default_database: Option<&str>,
    ) -> bool {
        let key = (instance.clone(), connection.clone());
        // Save the previous connection's last tab before switching.
        if self.active_connection.as_ref() != Some(&key) {
            self.save_connection_last_tab();
        }
        // 1) Restore the last-active tab for this connection if it still exists.
        if let Some(&global_idx) = self
            .connection_last_tab
            .get(&key)
            .filter(|&&i| i < self.tabs.len() && self.connection_key(&self.tabs[i].session) == key)
        {
            self.active_connection = Some(key);
            self.active_tab = Some(global_idx);
            return false;
        }
        // 2) Otherwise focus the last existing tab for this connection.
        let indices = self.visible_tab_indices_for(&key);
        if let Some(&global_idx) = indices.last() {
            self.active_connection = Some(key.clone());
            self.active_tab = Some(global_idx);
            self.connection_last_tab.insert(key, global_idx);
            return false;
        }
        // 3) No existing tab: open a new one.
        self.open_connection_tab(instance, connection, connection_id, database, schema, default_database);
        true
    }

    /// Global indices of tabs whose session matches `key`.
    fn visible_tab_indices_for(&self, key: &(String, String)) -> Vec<usize> {
        self.tabs
            .iter()
            .enumerate()
            .filter(|(_i, tab)| self.connection_key(&tab.session) == *key)
            .map(|(i, _tab)| i)
            .collect()
    }

    /// Open a new tab bound to a connection, carrying its display identity so
    /// history keys and query execution use real names. Sets this connection as
    /// the active one.
    pub fn open_connection_tab(
        &mut self,
        instance: String,
        connection: String,
        connection_id: String,
        database: Option<String>,
        schema: Option<String>,
        default_database: Option<&str>,
    ) {
        let key = (instance.clone(), connection.clone());
        // Enforce the per-connection tab limit (shared `at_tab_limit` check).
        if self.at_tab_limit(&key) {
            return;
        }
        // Save the previous connection's last tab before switching.
        if self.active_connection.as_ref() != Some(&key) {
            self.save_connection_last_tab();
        }
        self.active_connection = Some(key.clone());
        let id = self.next_session_id();
        let sequence = self.next_sequence(&key);
        // An explicit database/schema (e.g. from the objects tree) wins; when
        // `None` the new tab inherits the connection's current context from its
        // most recent tab, or falls back to `default_database` + "public" when
        // the connection has no tab yet. `n` and `Alt+t` share this rule.
        let (database, schema) =
            self.inherit_context_for(&key, database, schema, default_database);
        self.tabs.push(SqlTab {
            session: TabSession {
                id,
                sequence,
                connection_id: Some(connection_id),
                instance: Some(instance),
                connection: Some(connection),
                database,
                schema,
            },
            focus: SqlFocus::default(),
            upper_pane: SqlFocus::Editor,
            split_ratio: DEFAULT_SPLIT_RATIO,
            history_pane_width: DEFAULT_HISTORY_WIDTH,
            complete_table_names: false,
            editor: EditorState::default(),
            results: ResultsState::default(),
            history: HistoryState::default(),
        });
        self.active_tab = Some(self.tabs.len() - 1);
        // Record this tab as the connection's last-active tab so every
        // connection always has a current tab, letting new tabs inherit its
        // context reliably (case A fallback is only reached with no tab at all).
        self.connection_last_tab.insert(key, self.tabs.len() - 1);
    }

    /// Close the tab at global `idx`. The active index is repaired; closing the
    /// last remaining tab for the active connection leaves no visible tabs.
    pub fn close_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        // Capture the connection key before removing the tab so we can drop the
        // stale last-active-tab bookmark when the last tab for that key closes.
        let key = self.connection_key(&self.tabs[idx].session);
        self.tabs.remove(idx);

        // Drop the last-active-tab bookmark when no tabs remain for this
        // connection, so a later reopen restores it fresh.
        if !self.tabs.iter().any(|tab| self.connection_key(&tab.session) == key) {
            self.connection_last_tab.remove(&key);
        }

        // The workspace is per-connection (matching the original dbm): when the
        // active connection has no visible tab left, it shows an empty state
        // ("No query tabs for this connection") rather than jumping to another
        // connection's tab. `active_connection` stays put.
        let visible = self.visible_tab_indices();
        if visible.is_empty() {
            self.active_tab = None;
            return;
        }
        // Find a visible tab closest to the removed index.
        let target = if let Some(pos) = visible.iter().position(|&v| v >= idx) {
            visible[pos]
        } else {
            *visible.last().unwrap()
        };
        self.active_tab = Some(target);
        // Persist the new active tab for this connection.
        self.save_connection_last_tab();
    }

    /// Return the index of the tab whose stable `session.id` equals `tab_id`,
    /// or `None` if no such tab exists.
    pub fn index_of(&self, tab_id: usize) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.session.id == tab_id)
    }

    /// Allocate the next stable, globally-unique session id (used both when
    /// opening a tab and when restoring tabs from a persisted session).
    pub fn next_session_id(&mut self) -> usize {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_with_no_tabs() {
        let state = SqlTabState::default();
        assert!(state.tabs.is_empty(), "no tab should be auto-created");
        assert!(state.active_connection.is_none());
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
            None,
        );
        let session = &state.tabs[state.active_tab.unwrap()].session;
        assert_eq!(session.instance.as_deref(), Some("local"));
        assert_eq!(session.connection.as_deref(), Some("app-db"));
        assert_eq!(session.connection_id.as_deref(), Some("conn-42"));
        assert_eq!(session.database.as_deref(), Some("mydb"));
        assert_eq!(session.schema.as_deref(), Some("public"));
        assert_eq!(session.sequence, 1); // first tab for this connection
    }

    #[test]
    fn per_connection_sequence_increments() {
        let mut state = SqlTabState::default();
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        assert_eq!(state.tabs.last().unwrap().session.sequence, 1);
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        assert_eq!(state.tabs.last().unwrap().session.sequence, 2);

        // A different connection starts its own sequence at 1.
        state.open_connection_tab("local".into(), "c2".into(), "id2".into(), None, None, None);
        assert_eq!(state.tabs.last().unwrap().session.sequence, 1);
    }

    #[test]
    fn visible_tab_indices_filters_by_active_connection() {
        let mut state = SqlTabState::default();
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        state.open_connection_tab("local".into(), "c2".into(), "id2".into(), None, None, None);

        // After the last open_connection_tab, active_connection is c2.
        assert_eq!(state.visible_tab_indices(), vec![2]);
        assert_eq!(state.visible_tab_count(), 1);

        // Switch to c1.
        state.activate_connection("local".into(), "c1".into());
        assert_eq!(state.visible_tab_indices(), vec![0, 1]);
        assert_eq!(state.visible_tab_count(), 2);
    }

    #[test]
    fn activate_connection_restores_last_tab() {
        let mut state = SqlTabState::default();
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        // Second tab is active (index 1).
        assert_eq!(state.active_tab, Some(1));

        // Switch to c2.
        state.open_connection_tab("local".into(), "c2".into(), "id2".into(), None, None, None);

        // Switch back to c1 — should restore tab 1.
        state.activate_connection("local".into(), "c1".into());
        assert_eq!(state.active_tab, Some(1));
        assert_eq!(state.visible_tab_indices(), vec![0, 1]);
    }

    #[test]
    fn visible_to_global_maps_correctly() {
        let mut state = SqlTabState::default();
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        state.open_connection_tab("local".into(), "c2".into(), "id2".into(), None, None, None);

        // Active connection is c2 (one visible tab at global idx 2).
        assert_eq!(state.visible_to_global(0), Some(2));
        assert_eq!(state.visible_to_global(1), None);

        // Switch to c1 (two visible tabs at global idx 0, 1).
        state.activate_connection("local".into(), "c1".into());
        assert_eq!(state.visible_to_global(0), Some(0));
        assert_eq!(state.visible_to_global(1), Some(1));
        assert_eq!(state.visible_to_global(2), None);
    }

    #[test]
    fn global_to_visible_maps_correctly() {
        let mut state = SqlTabState::default();
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);

        assert_eq!(state.global_to_visible(), Some(1)); // second tab active

        // Open a tab for another connection.
        state.open_connection_tab("local".into(), "c2".into(), "id2".into(), None, None, None);
        // Active connection is now c2; the c2 tab is visible at offset 0.
        assert_eq!(state.global_to_visible(), Some(0));
    }

    #[test]
    fn index_of_finds_tab_by_session_id() {
        let mut state = SqlTabState::default();
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        let id = state.tabs[state.active_tab.unwrap()].session.id;
        assert_eq!(state.index_of(id), state.active_tab);
        assert_eq!(state.index_of(999_999), None);
    }

    #[test]
    fn session_ids_are_unique_across_connections_with_same_sequence() {
        // The original bug: session restore reused the per-connection `sequence`
        // as the global `session.id`, so two connections' Nth tab both got
        // `id == N`, colliding and making `index_of` route messages to the wrong
        // tab. Verify that freshly allocated ids stay globally unique even when
        // per-connection sequences coincide.
        let mut state = SqlTabState::default();
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        state.open_connection_tab("local".into(), "c2".into(), "id2".into(), None, None, None);
        state.open_connection_tab("local".into(), "c2".into(), "id2".into(), None, None, None);
        // Both connections have tabs sharing sequences (1, 2).
        assert_eq!(state.tabs[0].session.sequence, 1);
        assert_eq!(state.tabs[1].session.sequence, 2);
        assert_eq!(state.tabs[2].session.sequence, 1);
        assert_eq!(state.tabs[3].session.sequence, 2);
        // But global ids must be unique so `index_of` is unambiguous.
        let ids: Vec<usize> = state.tabs.iter().map(|t| t.session.id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "session ids must be globally unique");
    }

    #[test]
    fn sequence_resets_when_all_tabs_closed() {
        let mut state = SqlTabState::default();
        // Open two tabs for the same connection — sequences 1, 2.
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        assert_eq!(state.tabs[0].session.sequence, 1);
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        assert_eq!(state.tabs[1].session.sequence, 2);

        // Close both tabs.
        state.close_tab(1);
        state.close_tab(0);
        assert!(state.tabs.is_empty());

        // Re-open — sequence should restart at 1.
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        assert_eq!(state.tabs[0].session.sequence, 1);
    }

    #[test]
    fn focus_or_open_creates_tab_when_none_exists() {
        let mut state = SqlTabState::default();
        let created = state.focus_or_open_connection_tab(
            "local".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        assert!(created, "no existing tab should create one");
        assert_eq!(state.tabs.len(), 1);
        assert_eq!(
            state.active_connection.as_ref(),
            Some(&("local".to_string(), "c1".to_string()))
        );
    }

    #[test]
    fn focus_or_open_focuses_existing_tab_without_creating() {
        let mut state = SqlTabState::default();
        // Two tabs for c1, then a tab for c2 (so c1's last tab is recorded).
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        state.open_connection_tab("local".into(), "c2".into(), "id2".into(), None, None, None);
        let before = state.tabs.len();

        // Enter on c1 should focus its last tab (index 1) without a new tab.
        let created = state.focus_or_open_connection_tab(
            "local".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        assert!(!created, "existing tab should be focused, not created");
        assert_eq!(state.tabs.len(), before, "no new tab should be added");
        assert_eq!(state.active_tab, Some(1));
        assert_eq!(
            state.active_connection.as_ref(),
            Some(&("local".to_string(), "c1".to_string()))
        );
    }

    #[test]
    fn next_sequence_is_max_existing_plus_one() {
        let mut state = SqlTabState::default();
        // Open <sql 1>..<sql 6>.
        for _ in 0..6 {
            state.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        }
        assert_eq!(state.tabs.len(), 6);
        assert_eq!(state.tabs[5].session.sequence, 6);

        // Close <sql 2>..<sql 6> (global indices 1..=5), leaving only <sql 1>.
        // Closing a tab shifts the remaining ones left, so always close index 1
        // (the tab after <sql 1>).
        for _ in 0..5 {
            state.close_tab(1);
        }
        assert_eq!(state.tabs.len(), 1);
        assert_eq!(state.tabs[0].session.sequence, 1);

        // Reopen: the max existing sequence is 1, so the next tab is <sql 2>,
        // not a monotonic counter continuing at 7 (matching the original dbm).
        state.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        assert_eq!(state.tabs.len(), 2);
        assert_eq!(state.tabs[1].session.sequence, 2);
    }

    #[test]
    fn tab_limit_caps_at_max_tabs() {
        let mut state = SqlTabState::default();
        // Open up to MAX_TABS tabs.
        for _ in 0..MAX_TABS {
            state.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        }
        assert_eq!(state.tabs.len(), MAX_TABS);
        assert_eq!(state.tabs[state.tabs.len() - 1].session.sequence, MAX_TABS);
        // Attempting to open one more is a no-op at the limit.
        state.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        assert_eq!(state.tabs.len(), MAX_TABS, "must not exceed MAX_TABS tabs");
        // No sequence ever exceeds 9.
        assert!(state.tabs.iter().all(|t| t.session.sequence <= MAX_TABS));
    }

    #[test]
    fn open_tab_also_respects_tab_limit() {
        let mut state = SqlTabState::default();
        // Open MAX_TABS tabs for c1, then make c1 active.
        for _ in 0..MAX_TABS {
            state.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        }
        state.active_connection = Some(("inst".to_string(), "c1".to_string()));
        let before = state.tabs.len();
        // `open_tab` (the workspace `Alt+t` path) must hit the same limit.
        state.open_tab();
        assert_eq!(state.tabs.len(), before, "open_tab must respect MAX_TABS");
        assert!(state.tabs.iter().all(|t| t.session.sequence <= MAX_TABS));
    }

    #[test]
    fn open_tab_inherits_active_tab_context() {
        let mut state = SqlTabState::default();
        state.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            Some("mydb".into()),
            Some("public".into()),
            None,
        );
        state.active_connection = Some(("inst".to_string(), "c1".to_string()));
        // `Alt+t` opens a new tab within the same connection; it must inherit
        // the current tab's database/schema instead of starting blank `…/…`.
        state.open_tab();
        let new_tab = state.tabs.last().unwrap();
        assert_eq!(new_tab.session.database.as_deref(), Some("mydb"));
        assert_eq!(new_tab.session.schema.as_deref(), Some("public"));
    }

    #[test]
    fn open_connection_tab_inherits_context_when_none_passed() {
        let mut state = SqlTabState::default();
        // c1 has an existing tab scoped to a real database + schema.
        state.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            Some("mydb".into()),
            Some("public".into()),
            None,
        );
        // instances `n` passes `None` context; the new tab for the *same*
        // connection inherits c1's current context.
        state.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let new_tab = state.tabs.last().unwrap();
        assert_eq!(new_tab.session.database.as_deref(), Some("mydb"));
        assert_eq!(new_tab.session.schema.as_deref(), Some("public"));

        // A brand-new connection with no existing tab and no explicit
        // default_database: the database stays unset (`…/…`) but the schema
        // still falls back to `"public"`.
        state.open_connection_tab("inst".into(), "c2".into(), "id2".into(), None, None, None);
        let fresh = state.tabs.last().unwrap();
        assert_eq!(fresh.session.database, None);
        assert_eq!(fresh.session.schema.as_deref(), Some("public"));

        // Case A: a brand-new connection gets the connection's default database
        // (passed as `default_database`) + "public", matching the original dbm.
        state.open_connection_tab("inst".into(), "c3".into(), "id3".into(), None, None, Some("postgres"));
        let case_a = state.tabs.last().unwrap();
        assert_eq!(case_a.session.database.as_deref(), Some("postgres"));
        assert_eq!(case_a.session.schema.as_deref(), Some("public"));
    }

    #[test]
    fn new_tab_inherits_currently_active_tab_context() {
        // Reported scenario: c1 has tab1 (db1/s1, currently active) and tab2
        // (db2/s2). A new tab opened on c1 must inherit the *currently active*
        // tab's context (tab1), not the last-created tab (tab2).
        let mut state = SqlTabState::default();
        // tab1 -> db1/s1 (the last-active / current tab).
        state.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            Some("db1".into()),
            Some("s1".into()),
            None,
        );
        // tab2 -> db2/s2 (created later, becomes the last-created tab).
        state.open_tab();
        state.tabs.last_mut().unwrap().session.database = Some("db2".into());
        state.tabs.last_mut().unwrap().session.schema = Some("s2".into());
        // Switch back to tab1 (index 0) within c1; the active connection's
        // current-tab bookmark must follow (as the `Tab` handler does).
        state.active_tab = Some(0);
        state.remember_active_tab_for_connection();
        assert_eq!(state.active_tab, Some(0));

        // A new tab on c1 (`n`/`Alt+t`) must inherit tab1's context.
        state.open_tab();
        let new_tab = state.tabs.last().unwrap();
        assert_eq!(new_tab.session.database.as_deref(), Some("db1"));
        assert_eq!(new_tab.session.schema.as_deref(), Some("s1"));
    }

    #[test]
    fn closing_all_tabs_of_active_connection_shows_empty_workspace() {
        let mut state = SqlTabState::default();
        // A tab for c1 and a tab for c2.
        state.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        state.open_connection_tab("inst".into(), "c2".into(), "id2".into(), None, None, None);
        // Activate c1 (the first connection's tab).
        state.activate_connection("inst".into(), "c1".into());
        assert_eq!(state.active_tab, Some(0));
        assert_eq!(state.visible_tab_count(), 1);

        // Close c1's only tab. The workspace is per-connection: the active
        // connection stays c1 and becomes empty (active_tab = None) rather than
        // jumping to c2's tab.
        if let Some(global) = state.visible_to_global(0) {
            state.close_tab(global);
        }
        assert_eq!(state.tabs.len(), 1, "c2's tab must remain");
        assert_eq!(
            state.active_connection.as_ref(),
            Some(&("inst".to_string(), "c1".to_string())),
            "active connection must stay c1 (per-connection workspace)"
        );
        assert_eq!(state.active_tab, None, "c1 has no visible tab left");
        assert_eq!(state.visible_tab_count(), 0);
    }

    #[test]
    fn closing_last_tab_empties_tabs() {
        let mut state = SqlTabState::default();
        state.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        assert_eq!(state.tabs.len(), 1);
        // Close the only visible tab (visible offset 0).
        if let Some(global) = state.visible_to_global(0) {
            state.close_tab(global);
        }
        assert!(state.tabs.is_empty(), "closing the last tab must empty tabs");
        assert_eq!(state.active_tab, None);
    }

    #[test]
    fn enter_then_n_sequences_increment_continuously() {
        let mut state = SqlTabState::default();
        // Enter: focus-or-open creates the first tab (seq 1).
        let created = state.focus_or_open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        assert!(created);
        assert_eq!(state.tabs[0].session.sequence, 1);
        // n: always open fresh -> seq 2.
        state.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        assert_eq!(state.tabs.len(), 2);
        assert_eq!(state.tabs[1].session.sequence, 2);
        // n again -> seq 3.
        state.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        assert_eq!(state.tabs.len(), 3);
        assert_eq!(state.tabs[2].session.sequence, 3);
    }

    #[test]
    fn new_connection_tab_always_opens_fresh() {
        let mut state = SqlTabState::default();
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        let before = state.tabs.len();

        // `n` reuses the always-new open_connection_tab, so a third tab appears.
        state.open_connection_tab("local".into(), "c1".into(), "id1".into(), None, None, None);
        assert_eq!(state.tabs.len(), before + 1);
        assert_eq!(state.tabs.last().unwrap().session.sequence, 3);
    }
}
