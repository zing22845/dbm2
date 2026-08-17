//! Explorer objects feature messages.

use super::state::ObjectKind;

/// The actual objects (object tree) messages.
#[derive(Debug, Clone)]
pub enum ObjectsMessage {
    /// Move the cursor up.
    MoveUp,
    /// Move the cursor down.
    MoveDown,
    /// Move the cursor to a specific row (mouse click).
    JumpTo { row: usize },
    /// Toggle expand/collapse the expandable row at the given visible row
    /// (mouse marker click) without moving the cursor.
    ToggleExpandAt { row: usize },
    /// Expand the row under the cursor (`l`), matching the instances pane: it
    /// only expands, never collapses (`h` collapses). Unlike `Select` it never
    /// activates a schema or opens an object.
    Expand,
    /// Collapse the current row (`h`), matching the original dbm.
    Collapse,
    /// Scroll the tree horizontally by `delta` columns (`Left`/`Right`),
    /// matching the original dbm. `term_width` is the current terminal width
    /// (used to approximate the explorer viewport). No-op (dirty=false) at a
    /// scroll boundary.
    ScrollHorizontal { delta: i16, term_width: u16 },
    /// Activate the current row: expand/collapse an expandable row, otherwise
    /// open an object (e.g. a table) — matching the original dbm's `Enter`.
    Select,
    /// Rebind the tree to an instance/connection.
    Bind { instance: String, connection: String },
    /// The databases of the bound connection were loaded.
    DatabasesLoaded { databases: Vec<String> },
    /// Loading databases failed.
    DatabasesError { error: String },
    /// The schemas (and extensions) of a database were loaded.
    SchemasLoaded { database: String, schemas: Vec<String> },
    /// Loading schemas failed.
    SchemasError { database: String, error: String },
    /// The extensions of a database were loaded.
    ExtensionsLoaded { database: String, extensions: Vec<String> },
    /// Loading extensions failed.
    ExtensionsError { database: String, error: String },
    /// A schema-scoped object list was loaded.
    ObjectListLoaded {
        database: String,
        schema: String,
        kind: ObjectKind,
        items: Vec<String>,
    },
    /// Loading a schema-scoped object list failed.
    ObjectListError {
        database: String,
        schema: String,
        kind: ObjectKind,
        error: String,
    },
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone)]
pub enum ObjectsMsg {
    Message(ObjectsMessage),
}

impl From<ObjectsMessage> for ObjectsMsg {
    fn from(m: ObjectsMessage) -> Self {
        ObjectsMsg::Message(m)
    }
}
