//! Explorer objects (object tree) feature state.

use std::collections::HashSet;

/// A flattened, displayable row in the object tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectsRow {
    /// Indentation depth (0 = database, 1 = schema, 2 = group, 3 = object).
    pub depth: usize,
    /// Whether the row is currently expanded.
    pub expanded: bool,
    /// Whether the row can be expanded (a database/schema/group).
    pub expandable: bool,
    /// The row's display label.
    pub label: String,
    /// For object rows, the fully-qualified target used to open a table.
    pub target: Option<ObjectsTarget>,
}

/// A resolved tree target (used when opening an object, e.g. a table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectsTarget {
    pub database: String,
    pub schema: Option<String>,
    pub name: String,
}

/// State for the explorer objects pane.
///
/// The catalog rows are populated by the catalog-fetch effect (a later phase,
/// once a real connection is available). This phase wires the tree state,
/// navigation and selection.
#[derive(Debug, Clone, Default)]
pub struct ObjectsState {
    /// The flattened visible rows of the object tree.
    pub rows: Vec<ObjectsRow>,
    /// Cursor row within the tree.
    pub cursor: usize,
    /// Scroll offset.
    pub scroll: usize,
    /// The instance and connection the tree is bound to (empty = unbound).
    pub bound_instance: String,
    pub bound_connection: String,
    /// Expansion keys (database / database+schema) currently expanded.
    pub expanded: HashSet<String>,
}

impl ObjectsState {
    pub fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.cursor = (self.cursor + 1).min(self.rows.len() - 1);
    }

    /// Toggle expansion of the row under the cursor (databases/schemas/groups).
    pub fn toggle_expand(&mut self) {
        let Some(row) = self.rows.get(self.cursor) else {
            return;
        };
        if !row.expandable {
            return;
        }
        // Expansion state is driven by the catalog-fetch phase; toggling here
        // just flips the local flag until real rows are loaded.
        let key = row.target.as_ref().map(|t| t.database.clone()).unwrap_or_default();
        if !key.is_empty() {
            if self.expanded.contains(&key) {
                self.expanded.remove(&key);
            } else {
                self.expanded.insert(key);
            }
        }
    }

    /// The selected row's target (an object to open), if the cursor is on an
    /// object row.
    pub fn selected_target(&self) -> Option<ObjectsTarget> {
        self.rows.get(self.cursor).and_then(|r| r.target.clone())
    }

    /// Rebind the tree to a new instance/connection and reset navigation.
    pub fn rebind(&mut self, instance: String, connection: String) {
        self.bound_instance = instance;
        self.bound_connection = connection;
        self.cursor = 0;
        self.scroll = 0;
        self.expanded.clear();
    }
}
