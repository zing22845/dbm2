//! Context picker sub-module state.
//!
//! The picker is a database + schema selection overlay. It keeps the two
//! columns' search/cursor state plus the catalog lists loaded for the current
//! connection (`databases` and, per previewed database, `schemas`). Pure logic
//! and helpers live here so update and view stay thin.

use crate::common::components::search::PaneSearch;

/// Which picker column owns input (and cursor movement).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PickerColumn {
    #[default]
    Database,
    Schema,
}

/// The state of a single catalog list fetch (databases or schemas).
#[derive(Debug, Clone, Default)]
pub enum CachedList {
    #[default]
    Loading,
    Ready(Vec<String>),
    Error(String),
}

/// Interactive database/schema picker state.
#[derive(Debug, Clone, Default)]
pub struct ContextPickerState {
    /// Whether the picker overlay is open.
    pub open: bool,
    /// The focused column.
    pub column: PickerColumn,
    /// Cursor into the (filtered) database list.
    pub db_cursor: usize,
    /// Cursor into the (filtered) schema list.
    pub schema_cursor: usize,
    /// The database whose schemas are shown (preview).
    pub preview_database: String,
    /// The tab's current schema, used to seed the schema column cursor so it
    /// opens on the active schema rather than the first entry (matching dbm).
    pub preview_schema: String,
    /// The connection whose databases are listed.
    pub instance: String,
    /// The connection whose databases are listed.
    pub connection: String,
    /// Database column `/` search.
    pub db_search: PaneSearch,
    /// Schema column `/` search.
    pub schema_search: PaneSearch,
    /// The cached database list for the current connection.
    pub databases: CachedList,
    /// The cached schema list for the previewed database.
    pub schemas: CachedList,
}

impl ContextPickerState {
    /// Open a fresh picker focused on `column`, seeded with the current
    /// connection context (`instance`/`connection`) and the tab's active
    /// `database`/`schema` (so the preview and the cursor seed to the active
    /// context rather than the first entry).
    pub fn open(
        column: PickerColumn,
        instance: String,
        connection: String,
        database: String,
        schema: String,
    ) -> Self {
        ContextPickerState {
            open: true,
            column,
            instance,
            connection,
            preview_database: database,
            preview_schema: schema,
            ..ContextPickerState::default()
        }
    }

    /// Close the picker overlay.
    pub fn close(&mut self) {
        self.open = false;
        self.db_search.reset();
        self.schema_search.reset();
    }

    /// Whether either column is mid-search-input.
    pub fn search_input_active(&self) -> bool {
        self.db_search.active || self.schema_search.active
    }

    /// The active column's search, mutably.
    pub fn active_search_mut(&mut self) -> &mut PaneSearch {
        match self.column {
            PickerColumn::Database => &mut self.db_search,
            PickerColumn::Schema => &mut self.schema_search,
        }
    }

    /// Begin `/` search input on the active column.
    pub fn begin_search_input(&mut self) {
        self.db_search.end();
        self.schema_search.end();
        self.active_search_mut().reset();
        self.active_search_mut().start();
        match self.column {
            PickerColumn::Database => self.db_cursor = 0,
            PickerColumn::Schema => self.schema_cursor = 0,
        }
    }

    /// Cancel the active search input (clears its query).
    pub fn cancel_search_input(&mut self) {
        self.active_search_mut().reset();
    }

    /// Switch the focused column, preserving each column's query and cursor.
    /// Returns `true` if the column actually changed.
    pub fn switch_column(&mut self, column: PickerColumn) -> bool {
        if self.column == column {
            return false;
        }
        let was_searching = self.search_input_active();
        self.column = column;
        if was_searching {
            self.db_search.end();
            self.schema_search.end();
            self.active_search_mut().start();
        }
        true
    }
}

/// A sensible default schema: `public` when present, else the first entry.
pub fn default_schema(schemas: &[String]) -> String {
    if schemas.iter().any(|s| s == "public") {
        "public".into()
    } else {
        schemas.first().cloned().unwrap_or_else(|| "public".into())
    }
}

/// Indices into `items` matching the search query (all indices when no filter).
pub fn filter_indices(items: &[String], search: &PaneSearch) -> Vec<usize> {
    search.matching_indices(items)
}

/// Number of filtered items for a column title counter.
pub fn filtered_count(items: &[String], search: &PaneSearch) -> usize {
    filter_indices(items, search).len()
}

/// The filtered item at `cursor`, if any.
pub fn item_at_filtered(items: &[String], search: &PaneSearch, cursor: usize) -> Option<String> {
    let indices = filter_indices(items, search);
    indices.get(cursor).map(|&i| items[i].clone())
}

/// Clamp `cursor` into `[0, count)` (0 when empty).
pub fn clamp_cursor(cursor: usize, count: usize) -> usize {
    if count == 0 { 0 } else { cursor.min(count - 1) }
}

/// Cursor for the first filtered item equal to `name` (0 when not found).
pub fn cursor_for_name(items: &[String], search: &PaneSearch, name: &str) -> usize {
    let indices = filter_indices(items, search);
    indices.iter().position(|&i| items[i] == name).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_column_preserves_search_queries_and_cursors() {
        let mut picker = ContextPickerState {
            open: true,
            column: PickerColumn::Database,
            db_cursor: 2,
            schema_cursor: 1,
            preview_database: "myapp".into(),
            db_search: PaneSearch {
                active: true,
                query: "my".into(),
                ..PaneSearch::default()
            },
            schema_search: PaneSearch {
                query: "pub".into(),
                ..PaneSearch::default()
            },
            ..ContextPickerState::default()
        };
        picker.switch_column(PickerColumn::Schema);
        assert_eq!(picker.db_search.query, "my");
        assert_eq!(picker.schema_search.query, "pub");
        assert_eq!(picker.db_cursor, 2);
        assert_eq!(picker.schema_cursor, 1);
        assert!(picker.schema_search.active);
        assert!(!picker.db_search.active);
    }

    #[test]
    fn cancel_search_input_only_clears_active_column() {
        let mut picker = ContextPickerState {
            open: true,
            column: PickerColumn::Schema,
            db_cursor: 2,
            schema_cursor: 1,
            preview_database: "myapp".into(),
            db_search: PaneSearch {
                query: "keep".into(),
                ..PaneSearch::default()
            },
            schema_search: PaneSearch {
                active: true,
                query: "pub".into(),
                ..PaneSearch::default()
            },
            ..ContextPickerState::default()
        };
        picker.cancel_search_input();
        assert_eq!(picker.db_search.query, "keep");
        assert!(picker.schema_search.query.is_empty());
        assert!(!picker.schema_search.active);
        assert_eq!(picker.db_cursor, 2);
        assert_eq!(picker.schema_cursor, 1);
    }

    #[test]
    fn database_and_schema_filters_are_independent() {
        let dbs = vec!["myapp".into(), "postgres".into()];
        let schemas = vec!["public".into(), "analytics".into()];

        let mut db_search = PaneSearch {
            query: "myapp".into(),
            ..PaneSearch::default()
        };
        let db_filtered = filter_indices(&dbs, &db_search);
        assert_eq!(db_filtered.len(), 1);
        assert_eq!(dbs[db_filtered[0]], "myapp");

        let schema_search = PaneSearch::default();
        let schema_filtered = filter_indices(&schemas, &schema_search);
        assert_eq!(schema_filtered.len(), 2);

        db_search.query = "anal".into();
        let schema_filtered = filter_indices(&schemas, &db_search);
        assert_eq!(schema_filtered.len(), 1);
        assert_eq!(schemas[schema_filtered[0]], "analytics");
    }

    #[test]
    fn close_resets_open_and_search() {
        let mut picker = ContextPickerState::open(
            PickerColumn::Database,
            "inst".into(),
            "conn".into(),
            "postgres".into(),
            "public".into(),
        );
        picker.db_search.start();
        picker.db_search.query = "post".into();
        picker.close();
        assert!(!picker.open);
        assert!(!picker.db_search.active);
        assert!(picker.db_search.query.is_empty());
    }
}
