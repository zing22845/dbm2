//! Explorer objects feature update.

use super::effect::ObjectsEffect;
use super::intent::ObjectsIntent;
use super::msg::ObjectsMessage;
use super::state::{CatalogList, ObjectKind, ObjectsNode, ObjectsState};

/// Update the objects tree state. Pure by-value transition.
///
/// The returned `bool` is `dirty`: whether this update changed any state that
/// affects rendering. Navigation that ends up at a boundary (e.g. `MoveUp` at
/// the top row, or `ToggleExpand` on a leaf object) reports `false` so the
/// event loop can skip a redundant repaint.
pub fn update(
    msg: ObjectsMessage,
    mut state: ObjectsState,
) -> (ObjectsState, Vec<ObjectsIntent>, Vec<ObjectsEffect>, bool) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    let mut dirty = false;
    match msg {
        ObjectsMessage::MoveUp => dirty |= state.move_up(),
        ObjectsMessage::MoveDown => dirty |= state.move_down(),
        ObjectsMessage::ToggleExpand => {
            if let Some(node) = state.toggle_expand() {
                maybe_fetch_on_expand(&node, &state, &mut effects);
                dirty = true;
            }
        }
        ObjectsMessage::Select => {
            // Selecting an object (e.g. a table) notifies the SQL workspace to
            // open it. This is a cross-feature intent, consumed at the shell
            // layer; the objects state itself is unchanged here (the shell
            // marks the target workspace dirty when it opens the object).
            if let Some(target) = state.selected_target() {
                intents.push(ObjectsIntent::OpenObject { target });
            }
        }
        ObjectsMessage::Bind { instance, connection } => {
            state.rebind(instance.clone(), connection.clone());
            dirty = true;
            effects.push(ObjectsEffect::LoadDatabases { instance, connection });
        }
        ObjectsMessage::DatabasesLoaded { databases } => {
            state.catalog.databases = CatalogList::Ready(databases);
            state.rebuild_rows();
            dirty = true;
        }
        ObjectsMessage::DatabasesError { error } => {
            state.catalog.databases = CatalogList::Error(error);
            state.rebuild_rows();
            dirty = true;
        }
        ObjectsMessage::SchemasLoaded { database, schemas } => {
            state
                .catalog
                .schemas
                .insert(database, CatalogList::Ready(schemas));
            state.rebuild_rows();
            dirty = true;
        }
        ObjectsMessage::SchemasError { database, error } => {
            state
                .catalog
                .schemas
                .insert(database, CatalogList::Error(error));
            state.rebuild_rows();
            dirty = true;
        }
        ObjectsMessage::ExtensionsLoaded { database, extensions } => {
            state
                .catalog
                .extensions
                .insert(database, CatalogList::Ready(extensions));
            state.rebuild_rows();
            dirty = true;
        }
        ObjectsMessage::ExtensionsError { database, error } => {
            state
                .catalog
                .extensions
                .insert(database, CatalogList::Error(error));
            state.rebuild_rows();
            dirty = true;
        }
        ObjectsMessage::ObjectListLoaded { database, schema, kind, items } => {
            state.catalog.objects.insert(
                (database, schema, kind),
                CatalogList::Ready(items),
            );
            state.rebuild_rows();
            dirty = true;
        }
        ObjectsMessage::ObjectListError { database, schema, kind, error } => {
            state.catalog.objects.insert(
                (database, schema, kind),
                CatalogList::Error(error),
            );
            state.rebuild_rows();
            dirty = true;
        }
    }
    (state, intents, effects, dirty)
}

/// Decide which catalog effect to kick off when `node` is being expanded.
///
/// Databases fetch their schemas (extensions are fetched on their own group's
/// expand); schemas leave their group children to fetch lazily; groups fetch
/// their own object list. Only fires when the requested level has not already
/// been fetched.
fn maybe_fetch_on_expand(
    node: &ObjectsNode,
    state: &ObjectsState,
    effects: &mut Vec<ObjectsEffect>,
) {
    if state.bound_instance.is_empty() || state.bound_connection.is_empty() {
        return;
    }
    let instance = state.bound_instance.clone();
    let connection = state.bound_connection.clone();
    match node {
        ObjectsNode::Database { name } => {
            if !state.catalog.schemas.contains_key(name) {
                effects.push(ObjectsEffect::LoadSchemas {
                    instance,
                    connection,
                    database: name.clone(),
                });
            }
        }
        ObjectsNode::Schema { database, name } => {
            // Child groups fetch their lists lazily; nothing to load here.
            let _ = (database, name);
        }
        ObjectsNode::Group { database, schema, kind } => match kind {
            ObjectKind::Extensions => {
                if !state.catalog.extensions.contains_key(database) {
                    effects.push(ObjectsEffect::LoadExtensions {
                        instance,
                        connection,
                        database: database.clone(),
                    });
                }
            }
            other => {
                if let Some(schema) = schema {
                    let list_key = (database.clone(), schema.clone(), *other);
                    if !state.catalog.objects.contains_key(&list_key) {
                        effects.push(ObjectsEffect::LoadObjectList {
                            instance,
                            connection,
                            database: database.clone(),
                            schema: schema.clone(),
                            kind: *other,
                        });
                    }
                }
            }
        },
        ObjectsNode::Object { .. } => {}
    }
}
