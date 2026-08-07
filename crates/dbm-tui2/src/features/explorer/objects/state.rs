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
    /// The instance and connection the tree is bound to (empty = unbound).
    pub bound_instance: String,
    pub bound_connection: String,
    /// Expansion keys (database / database+schema / database+group).
    pub expanded: HashSet<String>,
}

impl ObjectsState {
    fn expand_key_database(database: &str) -> String {
        database.to_string()
    }

    fn expand_key_schema(database: &str, schema: &str) -> String {
        format!("{database}\t{schema}")
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
    /// `None` when the row is not expandable.
    pub fn toggle_expand(&mut self) -> Option<ObjectsNode> {
        let node = self.node_at_cursor()?.clone();
        if matches!(node, ObjectsNode::Object { .. }) {
            return None;
        }
        let key = self.expand_key_of(&node);
        if self.expanded.contains(&key) {
            self.expanded.remove(&key);
        } else {
            self.expanded.insert(key);
        }
        self.rebuild_rows();
        Some(node)
    }

    /// Rebind the tree to a new instance/connection, reset navigation and
    /// catalog.
    pub fn rebind(&mut self, instance: String, connection: String) {
        self.bound_instance = instance;
        self.bound_connection = connection;
        self.cursor = 0;
        self.scroll = 0;
        self.expanded.clear();
        self.catalog = ObjectsCatalog::default();
        self.rows.clear();
    }

    /// Re-derive the flattened rows from the catalog and expansion set, then
    /// clamp the cursor and scroll to the new row count.
    pub fn rebuild_rows(&mut self) {
        self.rows = build_rows(
            &self.catalog,
            &self.expanded,
            &self.bound_instance,
            &self.bound_connection,
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
pub fn build_rows(
    catalog: &ObjectsCatalog,
    expanded: &HashSet<String>,
    _instance: &str,
    _connection: &str,
) -> Vec<ObjectsRow> {
    let mut rows = Vec::new();
    match &catalog.databases {
        CatalogList::NotFetched => {}
        CatalogList::Loading => rows.push(status_row(0, "Loading databases…")),
        CatalogList::Error(err) => rows.push(status_row(0, &format!("Error: {err}"))),
        CatalogList::Ready(databases) => {
            for database in databases {
                let db_key = ObjectsState::expand_key_database(database);
                let db_expanded = expanded.contains(&db_key);
                rows.push(ObjectsRow {
                    depth: 0,
                    expanded: db_expanded,
                    expandable: true,
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
                            let schema_expanded = expanded.contains(&schema_key);
                            rows.push(ObjectsRow {
                                depth: 1,
                                expanded: schema_expanded,
                                expandable: true,
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
