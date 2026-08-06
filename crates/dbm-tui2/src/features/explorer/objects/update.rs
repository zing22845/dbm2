//! Explorer objects feature update.

use super::effect::ObjectsEffect;
use super::intent::ObjectsIntent;
use super::msg::ObjectsMessage;
use super::state::{CatalogList, ObjectKind, ObjectsNode, ObjectsState};

/// Update the objects tree state. Pure by-value transition.
pub fn update(
    msg: ObjectsMessage,
    mut state: ObjectsState,
) -> (ObjectsState, Vec<ObjectsIntent>, Vec<ObjectsEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    match msg {
        ObjectsMessage::MoveUp => state.move_up(),
        ObjectsMessage::MoveDown => state.move_down(),
        ObjectsMessage::ToggleExpand => {
            if let Some(node) = state.toggle_expand() {
                maybe_fetch_on_expand(&node, &state, &mut effects);
            }
        }
        ObjectsMessage::Select => {
            // Selecting an object (e.g. a table) notifies the SQL workspace to
            // open it. This is a cross-feature intent, consumed at the shell
            // layer.
            if let Some(target) = state.selected_target() {
                intents.push(ObjectsIntent::OpenObject { target });
            }
        }
        ObjectsMessage::Bind { instance, connection } => {
            state.rebind(instance.clone(), connection.clone());
            effects.push(ObjectsEffect::LoadDatabases { instance, connection });
        }
        ObjectsMessage::DatabasesLoaded { databases } => {
            state.catalog.databases = CatalogList::Ready(databases);
            state.rebuild_rows();
        }
        ObjectsMessage::DatabasesError { error } => {
            state.catalog.databases = CatalogList::Error(error);
            state.rebuild_rows();
        }
        ObjectsMessage::SchemasLoaded { database, schemas } => {
            state
                .catalog
                .schemas
                .insert(database, CatalogList::Ready(schemas));
            state.rebuild_rows();
        }
        ObjectsMessage::SchemasError { database, error } => {
            state
                .catalog
                .schemas
                .insert(database, CatalogList::Error(error));
            state.rebuild_rows();
        }
        ObjectsMessage::ExtensionsLoaded { database, extensions } => {
            state
                .catalog
                .extensions
                .insert(database, CatalogList::Ready(extensions));
            state.rebuild_rows();
        }
        ObjectsMessage::ExtensionsError { database, error } => {
            state
                .catalog
                .extensions
                .insert(database, CatalogList::Error(error));
            state.rebuild_rows();
        }
        ObjectsMessage::ObjectListLoaded { database, schema, kind, items } => {
            state.catalog.objects.insert(
                (database, schema, kind),
                CatalogList::Ready(items),
            );
            state.rebuild_rows();
        }
        ObjectsMessage::ObjectListError { database, schema, kind, error } => {
            state.catalog.objects.insert(
                (database, schema, kind),
                CatalogList::Error(error),
            );
            state.rebuild_rows();
        }
    }
    (state, intents, effects)
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
