//! Explorer objects (object tree) feature state.

use std::collections::{HashMap, HashSet};

/// A lazily-fetched catalog list at one level of the tree.
///
/// The tree fetches each level on demand (databases on bind, schemas on
/// database expand, object lists on group expand) and stores the result here
/// so the rendered rows are derived purely from this cache plus the expansion
/// set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CatalogList {
    /// Not yet requested.
    #[default]
    NotFetched,
    /// A fetch is in flight.
    Loading,
    /// Fetched successfully.
    Ready(Vec<String>),
    /// The fetch failed.
    Error(String),
}

impl CatalogList {
    /// Whether the list has been fetched successfully.
    pub fn is_ready(&self) -> bool {
        matches!(self, CatalogList::Ready(_))
    }
}

/// The kind of objects shown in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    /// Database-scoped extensions.
    Extensions,
    Tables,
    Views,
    Matviews,
    Procedures,
    Functions,
    Sequences,
}

impl ObjectKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Extensions => "Extensions",
            Self::Tables => "Tables",
            Self::Views => "Views",
            Self::Matviews => "Materialized views",
            Self::Procedures => "Procedures",
            Self::Functions => "Functions",
            Self::Sequences => "Sequences",
        }
    }

    /// The schema-scoped kinds (all except extensions).
    pub fn schema_kinds() -> &'static [ObjectKind] {
        &[
            Self::Tables,
            Self::Views,
            Self::Matviews,
            Self::Procedures,
            Self::Functions,
            Self::Sequences,
        ]
    }
}

/// The catalog for the bound connection: databases, per-database schemas and
/// extensions, and per-(database, schema, kind) object lists.
#[derive(Debug, Clone, Default)]
pub struct ObjectsCatalog {
    /// Databases of the connection.
    pub databases: CatalogList,
    /// Schemas, keyed by database name.
    pub schemas: HashMap<String, CatalogList>,
    /// Extensions, keyed by database name.
    pub extensions: HashMap<String, CatalogList>,
    /// Object lists, keyed by (database, schema, kind).
    pub objects: HashMap<(String, String, ObjectKind), CatalogList>,
}

/// A node in the object tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectsNode {
    Database { name: String },
    Schema { database: String, name: String },
    Group { database: String, schema: Option<String>, kind: ObjectKind },
    Object { database: String, schema: Option<String>, kind: ObjectKind, name: String },
}

/// A flattened, displayable row in the object tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectsRow {
    /// Indentation depth (0 = database, 1 = schema/group, 2 = object).
    pub depth: usize,
    /// Whether the row is currently expanded.
    pub expanded: bool,
    /// Whether the row can be expanded (a database/schema/group).
    pub expandable: bool,
    /// Whether the row is the active schema (the schema of the currently-open
    /// SQL tab for the bound connection). Active schemas and their parent
    /// databases are highlighted and cannot be collapsed.
    pub active: bool,
    /// The row's node in the tree.
    pub node: ObjectsNode,
    /// The row's display label.
    pub label: String,
}

/// A resolved tree target (used when opening an object, e.g. a table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectsTarget {
    pub database: String,
    pub schema: Option<String>,
    pub name: String,
}

/// State for the explorer objects pane.
#[derive(Debug, Clone, Default)]
pub struct ObjectsState {
    /// The fetched catalog for the bound connection.
    pub catalog: ObjectsCatalog,
    /// The flattened visible rows of the object tree.
    pub rows: Vec<ObjectsRow>,
    /// Cursor row within the tree.
    pub cursor: usize,
    /// Scroll offset.
    pub scroll: usize,
    /// Horizontal scroll offset of the tree (`Left`/`Right`), matching the
    /// original dbm's objects horizontal scroll.
    pub h_scroll: u16,
    /// The instance and connection the tree is bound to (empty = unbound).
    pub bound_instance: String,
    pub bound_connection: String,
    /// Expansion keys (database / database+schema / database+group).
    pub expanded: HashSet<String>,
    /// Expansion keys restored from a session snapshot, to be re-applied when
    /// the tree is next bound to the matching connection. Empty when there is
    /// nothing pending.
    pub restore_expanded: Vec<String>,
    /// The connection the restore keys belong to (empty = no pending restore).
    pub restore_bound_connection: String,
    /// The active schema of the currently-open SQL tab for the bound connection,
    /// and its parent database. Active schemas (and their parent database) are
    /// highlighted and forced expanded; they cannot be collapsed.
    pub active_db: Option<String>,
    pub active_schema: Option<String>,
}

impl ObjectsState {
    fn expand_key_database(database: &str) -> String {
        database.to_string()
    }

    fn expand_key_schema(database: &str, schema: &str) -> String {
        format!("{database}\t{schema}")
    }

    /// Whether an expansion key belongs to the active path (the active database
    /// or the active schema), which is forced expanded (original dbm's
    /// `active_path_forces_expanded`).
    pub fn active_path_forces_expanded(
        key: &str,
        active_db: Option<&str>,
        active_schema: Option<&str>,
    ) -> bool {
        if let Some(db) = active_db {
            if key == Self::expand_key_database(db) {
                return true;
            }
        }
        if let (Some(db), Some(schema)) = (active_db, active_schema) {
            if key == Self::expand_key_schema(db, schema) {
                return true;
            }
        }
        false
    }

    /// Whether collapsing a node is blocked because it is on the active path
    /// (active database or active schema). Groups/objects are never blocked,
    /// matching the original dbm.
    pub fn collapse_blocked_by_active(
        node: &ObjectsNode,
        active_db: Option<&str>,
        active_schema: Option<&str>,
    ) -> bool {
        match node {
            ObjectsNode::Database { name } => active_db == Some(name.as_str()),
            ObjectsNode::Schema { database, name } => {
                active_db == Some(database.as_str()) && active_schema == Some(name.as_str())
            }
            _ => false,
        }
    }

    /// Set the active schema (and its parent database) for the bound connection.
    /// Empty strings clear the active state. After a change the tree is
    /// rebuilt so the active path is forced expanded.
    pub fn set_active(&mut self, database: Option<String>, schema: Option<String>) {
        let db = database.filter(|d| !d.is_empty());
        let sc = schema.filter(|s| !s.is_empty());
        if self.active_db == db && self.active_schema == sc {
            return;
        }
        self.active_db = db;
        self.active_schema = sc;
        self.rebuild_rows();
    }

    fn expand_key_group(database: &str, schema: Option<&str>, kind: ObjectKind) -> String {
        match schema {
            Some(schema) => format!("{database}\t{schema}\t{}", kind.label()),
            None => format!("{database}\t{}", kind.label()),
        }
    }

    /// The expansion key of a node (empty for object leaves, which cannot
    /// expand).
    pub fn expand_key_of(&self, node: &ObjectsNode) -> String {
        match node {
            ObjectsNode::Database { name } => Self::expand_key_database(name),
            ObjectsNode::Schema { database, name } => Self::expand_key_schema(database, name),
            ObjectsNode::Group { database, schema, kind } => {
                Self::expand_key_group(database, schema.as_deref(), *kind)
            }
            ObjectsNode::Object { .. } => String::new(),
        }
    }

    /// The node under the cursor, if any.
    pub fn node_at_cursor(&self) -> Option<&ObjectsNode> {
        self.rows.get(self.cursor).map(|r| &r.node)
    }

    /// The selected row's target (an object to open), if the cursor is on an
    /// object row.
    pub fn selected_target(&self) -> Option<ObjectsTarget> {
        let row = self.rows.get(self.cursor)?;
        match &row.node {
            ObjectsNode::Object { database, schema, name, .. } => Some(ObjectsTarget {
                database: database.clone(),
                schema: schema.clone(),
                name: name.clone(),
            }),
            _ => None,
        }
    }

    /// Move the cursor up (clamped). Returns whether the cursor actually moved,
    /// so callers can avoid repainting a no-op navigation.
    pub fn move_up(&mut self) -> bool {
        let before = self.cursor;
        self.cursor = self.cursor.saturating_sub(1);
        self.cursor != before
    }

    /// Jump the cursor to a specific row (mouse click), clamped to the visible
    /// rows. Returns whether the cursor moved.
    pub fn jump_to(&mut self, row: usize) -> bool {
        if self.rows.is_empty() {
            return false;
        }
        let target = row.min(self.rows.len() - 1);
        let before = self.cursor;
        self.cursor = target;
        self.cursor != before
    }

    /// Move the cursor down (clamped). Returns whether the cursor actually
    /// moved, so callers can avoid repainting a no-op navigation.
    pub fn move_down(&mut self) -> bool {
        if self.rows.is_empty() {
            return false;
        }
        let before = self.cursor;
        self.cursor = (self.cursor + 1).min(self.rows.len() - 1);
        self.cursor != before
    }

    /// Toggle the expansion of the row under the cursor. Returns the node that
    /// was toggled (so the caller can decide which catalog fetch to trigger);
    /// `None` when the row is not expandable or collapsing would violate the
    /// active-path constraint.
    pub fn toggle_expand(&mut self) -> Option<ObjectsNode> {
        let node = self.node_at_cursor()?.clone();
        if matches!(node, ObjectsNode::Object { .. }) {
            return None;
        }
        let key = self.expand_key_of(&node);
        if self.expanded.contains(&key) {
            // Collapsing an active database/schema is blocked (it is forced
            // expanded), matching the original dbm.
            if Self::collapse_blocked_by_active(
                &node,
                self.active_db.as_deref(),
                self.active_schema.as_deref(),
            ) {
                return None;
            }
            self.expanded.remove(&key);
        } else {
            self.expanded.insert(key);
        }
        self.rebuild_rows();
        Some(node)
    }

    /// Collapse the row under the cursor (if it is expanded), matching the
    /// original dbm's `h` key. Returns whether anything was collapsed.
    /// Collapsing an active database/schema is a no-op (it is forced expanded).
    pub fn collapse(&mut self) -> bool {
        let Some(node) = self.node_at_cursor().cloned() else {
            return false;
        };
        if Self::collapse_blocked_by_active(
            &node,
            self.active_db.as_deref(),
            self.active_schema.as_deref(),
        ) {
            return false;
        }
        let key = self.expand_key_of(&node);
        if key.is_empty() {
            return false;
        }
        if self.expanded.remove(&key) {
            self.rebuild_rows();
            true
        } else {
            false
        }
    }

    /// The display width (in columns) of the widest rendered row. Used to
    /// clamp horizontal scrolling so it stops at the content boundary.
    pub fn max_row_width(&self) -> u16 {
        self.rows
            .iter()
            .map(|r| {
                // Each row renders as "{indent}{marker} {label}"
                // where indent = depth * 2 spaces, marker = ▸/▾/─/·
                let indent = r.depth.saturating_mul(2) as usize;
                let marker_w = 1usize; // ▸/▾/─/· = 1 col each
                let label_w = unicode_width::UnicodeWidthStr::width(r.label.as_str());
                let total: usize = indent + marker_w + 1 + label_w; // +1 for space after marker
                total.try_into().unwrap_or(u16::MAX)
            })
            .max()
            .unwrap_or(0)
    }

    /// Scroll the tree horizontally by `delta` columns, clamped to `[0, max]`.
    /// Returns whether the offset moved, so a no-op at a boundary skips a
    /// redundant repaint (matching the original dbm).
    pub fn scroll_horizontal(&mut self, delta: i16, max: u16) -> bool {
        let before = self.h_scroll;
        self.h_scroll = (self.h_scroll as i32 + i32::from(delta))
            .clamp(0, i32::from(max)) as u16;
        self.h_scroll != before
    }

    /// Keep the tree bound to the active connection. This is idempotent: when
    /// the binding is already up to date nothing is reset (so catalog/rows
    /// survive), otherwise it rebinds. Returns whether the binding changed.
    pub fn sync_binding(&mut self, instance: String, connection: String) -> bool {
        if self.bound_instance == instance && self.bound_connection == connection {
            return false;
        }
        self.rebind(instance, connection);
        true
    }

    /// Unbind the tree (active workspace is an instance, not a connection), so
    /// the objects pane shows the "open a connection to browse objects" prompt.
    /// Returns whether the binding changed.
    pub fn clear_binding(&mut self) -> bool {
        if self.bound_instance.is_empty() && self.bound_connection.is_empty() {
            return false;
        }
        self.bound_instance.clear();
        self.bound_connection.clear();
        self.cursor = 0;
        self.scroll = 0;
        self.h_scroll = 0;
        self.expanded.clear();
        self.catalog = ObjectsCatalog::default();
        self.rows.clear();
        true
    }

    /// Rebind the tree to a new instance/connection, reset navigation and
    /// catalog. If a session restore left expansion keys for this connection,
    /// they are re-applied so the tree comes back expanded where the user left
    /// it.
    pub fn rebind(&mut self, instance: String, connection: String) {
        let matches_restore = self.restore_bound_connection == connection;
        self.bound_instance = instance;
        self.bound_connection = connection;
        self.cursor = 0;
        self.scroll = 0;
        self.h_scroll = 0;
        self.expanded.clear();
        if matches_restore {
            for key in std::mem::take(&mut self.restore_expanded) {
                self.expanded.insert(key);
            }
            self.restore_bound_connection.clear();
        }
        self.catalog = ObjectsCatalog::default();
        self.rows.clear();
    }

    /// Re-derive the flattened rows from the catalog and expansion set, then
    /// clamp the cursor and scroll to the new row count.
    pub fn rebuild_rows(&mut self) {
        self.rows = build_rows(
            &self.catalog,
            &self.expanded,
            self.active_db.as_deref(),
            self.active_schema.as_deref(),
        );
        if self.rows.is_empty() {
            self.cursor = 0;
            self.scroll = 0;
            return;
        }
        self.cursor = self.cursor.min(self.rows.len() - 1);
        let max_scroll = self.rows.len().saturating_sub(1);
        self.scroll = self.scroll.min(max_scroll);
    }
}

/// Build the flat list of visible tree rows from the catalog and the expansion
/// set. Pure function: it reads the catalog and returns new rows without
/// mutating any state.
///
/// `active_db`/`active_schema` (the active SQL tab's database/schema for the
/// bound connection) force their path open and mark the active schema row,
/// matching the original dbm.
pub fn build_rows(
    catalog: &ObjectsCatalog,
    expanded: &HashSet<String>,
    active_db: Option<&str>,
    active_schema: Option<&str>,
) -> Vec<ObjectsRow> {
    let mut rows = Vec::new();
    match &catalog.databases {
        CatalogList::NotFetched => {}
        CatalogList::Loading => rows.push(status_row(0, "Loading databases…")),
        CatalogList::Error(err) => rows.push(status_row(0, &format!("Error: {err}"))),
        CatalogList::Ready(databases) => {
            for database in databases {
                let db_key = ObjectsState::expand_key_database(database);
                // The active database is forced expanded even if not in the set.
                let db_expanded = ObjectsState::active_path_forces_expanded(
                    &db_key, active_db, active_schema,
                ) || expanded.contains(&db_key);
                rows.push(ObjectsRow {
                    depth: 0,
                    expanded: db_expanded,
                    expandable: true,
                    active: false,
                    node: ObjectsNode::Database {
                        name: database.clone(),
                    },
                    label: database.clone(),
                });
                if !db_expanded {
                    continue;
                }
                push_group_rows(
                    &mut rows,
                    catalog,
                    expanded,
                    database,
                    None,
                    ObjectKind::Extensions,
                    1,
                );
                match catalog.schemas.get(database) {
                    None | Some(CatalogList::NotFetched) => {}
                    Some(CatalogList::Loading) => rows.push(status_row(1, "Schemas (Loading…)")),
                    Some(CatalogList::Error(err)) => {
                        rows.push(status_row(1, &format!("Schemas (Error: {err})")))
                    }
                    Some(CatalogList::Ready(schemas)) => {
                        for schema in schemas {
                            let schema_key = ObjectsState::expand_key_schema(database, schema);
                            // The active schema is forced expanded and highlighted.
                            let schema_active =
                                active_db == Some(database.as_str())
                                    && active_schema == Some(schema.as_str());
                            let schema_expanded = schema_active
                                || ObjectsState::active_path_forces_expanded(
                                    &schema_key, active_db, active_schema,
                                )
                                || expanded.contains(&schema_key);
                            rows.push(ObjectsRow {
                                depth: 1,
                                expanded: schema_expanded,
                                expandable: true,
                                active: schema_active,
                                node: ObjectsNode::Schema {
                                    database: database.clone(),
                                    name: schema.clone(),
                                },
                                label: schema.clone(),
                            });
                            if !schema_expanded {
                                continue;
                            }
                            for kind in ObjectKind::schema_kinds() {
                                push_group_rows(
                                    &mut rows,
                                    catalog,
                                    expanded,
                                    database,
                                    Some(schema),
                                    *kind,
                                    2,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    rows
}

fn status_row(depth: usize, label: &str) -> ObjectsRow {
    ObjectsRow {
        depth,
        expanded: false,
        expandable: false,
        active: false,
        node: ObjectsNode::Database {
            name: String::new(),
        },
        label: label.to_string(),
    }
}

fn push_group_rows(
    rows: &mut Vec<ObjectsRow>,
    catalog: &ObjectsCatalog,
    expanded: &HashSet<String>,
    database: &str,
    schema: Option<&str>,
    kind: ObjectKind,
    depth: usize,
) {
    let group_key = ObjectsState::expand_key_group(database, schema, kind);
    let group_expanded = expanded.contains(&group_key);
    let list = match kind {
        ObjectKind::Extensions => catalog.extensions.get(database),
        other => {
            let Some(schema) = schema else {
                return;
            };
            catalog.objects.get(&(database.to_string(), schema.to_string(), other))
        }
    };
    let label = match list {
        None | Some(CatalogList::NotFetched) => kind.label().to_string(),
        Some(CatalogList::Loading) => format!("{} (Loading…)", kind.label()),
        Some(CatalogList::Error(err)) => format!("{} (Error: {err})", kind.label()),
        Some(CatalogList::Ready(items)) => format!("{} ({})", kind.label(), items.len()),
    };
    rows.push(ObjectsRow {
        depth,
        expanded: group_expanded,
        expandable: true,
        active: false,
        node: ObjectsNode::Group {
            database: database.to_string(),
            schema: schema.map(str::to_string),
            kind,
        },
        label,
    });
    if !group_expanded {
        return;
    }
    if let Some(CatalogList::Ready(items)) = list {
        for name in items {
            rows.push(ObjectsRow {
                depth: depth + 1,
                expanded: false,
                expandable: false,
                active: false,
                node: ObjectsNode::Object {
                    database: database.to_string(),
                    schema: schema.map(str::to_string),
                    kind,
                    name: name.clone(),
                },
                label: name.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_with(schemas: &[&str]) -> ObjectsCatalog {
        let mut c = ObjectsCatalog::default();
        c.databases = CatalogList::Ready(vec!["db".to_string()]);
        c.schemas.insert(
            "db".to_string(),
            CatalogList::Ready(schemas.iter().map(|s| s.to_string()).collect()),
        );
        c
    }

    #[test]
    fn build_rows_marks_active_schema_and_forces_expansion() {
        let c = catalog_with(&["public", "other"]);
        // Empty expansion set; active path db/public is forced open.
        let rows = build_rows(&c, &HashSet::new(), Some("db"), Some("public"));
        let schema_rows: Vec<&ObjectsRow> = rows.iter().filter(|r| matches!(r.node, ObjectsNode::Schema { .. })).collect();
        assert_eq!(schema_rows.len(), 2);
        let public = schema_rows.iter().find(|r| r.label == "public").unwrap();
        assert!(public.active, "public must be the active schema");
        assert!(public.expanded, "active schema is forced expanded");
        let other = schema_rows.iter().find(|r| r.label == "other").unwrap();
        assert!(!other.active);
        assert!(!other.expanded, "inactive schema stays collapsed");
    }

    #[test]
    fn collapse_blocked_for_active_path_not_groups() {
        // Active database and schema cannot be collapsed.
        assert!(ObjectsState::collapse_blocked_by_active(
            &ObjectsNode::Database { name: "db".into() },
            Some("db"),
            Some("public"),
        ));
        assert!(ObjectsState::collapse_blocked_by_active(
            &ObjectsNode::Schema { database: "db".into(), name: "public".into() },
            Some("db"),
            Some("public"),
        ));
        // A group under the active schema is NOT blocked.
        assert!(!ObjectsState::collapse_blocked_by_active(
            &ObjectsNode::Group {
                database: "db".into(),
                schema: Some("public".into()),
                kind: ObjectKind::Tables,
            },
            Some("db"),
            Some("public"),
        ));
    }

    #[test]
    fn jump_to_moves_and_clamps_cursor() {
        let mut s = ObjectsState::default();
        s.rows = vec![
            ObjectsRow { depth: 0, expanded: false, expandable: true, active: false, node: ObjectsNode::Database { name: "a".into() }, label: "a".into() },
            ObjectsRow { depth: 0, expanded: false, expandable: true, active: false, node: ObjectsNode::Database { name: "b".into() }, label: "b".into() },
        ];
        s.jump_to(1);
        assert_eq!(s.cursor, 1);
        // Clamp to the last row (index 1) — no movement since already there.
        assert!(!s.jump_to(100));
        assert_eq!(s.cursor, 1);
        assert!(!s.jump_to(1));
        // Empty rows -> no-op.
        let mut e = ObjectsState::default();
        assert!(!e.jump_to(0));
    }

    #[test]
    fn sync_binding_is_idempotent_and_clear_unbinds() {
        let mut s = ObjectsState::default();
        // First sync rebinds and reports a change.
        assert!(s.sync_binding("inst".into(), "conn".into()));
        assert_eq!(s.bound_instance, "inst");
        assert_eq!(s.bound_connection, "conn");
        // Same binding -> no change (catalog/rows preserved).
        assert!(!s.sync_binding("inst".into(), "conn".into()));
        // Different binding -> change.
        assert!(s.sync_binding("inst".into(), "other".into()));
        // Clear unbinds.
        assert!(s.clear_binding());
        assert!(s.bound_connection.is_empty());
        // Clear again -> no change.
        assert!(!s.clear_binding());
    }

    #[test]
    fn set_active_clears_when_empty() {
        let mut s = ObjectsState::default();
        s.set_active(Some("db".into()), Some("public".into()));
        assert_eq!(s.active_db.as_deref(), Some("db"));
        assert_eq!(s.active_schema.as_deref(), Some("public"));
        s.set_active(None, None);
        assert!(s.active_db.is_none());
        assert!(s.active_schema.is_none());
    }
}
