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
        ObjectsMessage::Collapse => {
            dirty |= state.collapse();
        }
        ObjectsMessage::ScrollHorizontal { delta, term_width } => {
            // The explorer takes ~20% of terminal width.  Clamp so `h_scroll`
            // never grows past the longest content row beyond the viewport.
            // When content fits fully inside the viewport, `max` is 0 and
            // pressing Right is a no-op — matching the original dbm.
            let viewport_w = (term_width as u32 * 20 / 100).max(1) as u16;
            let max = state.max_row_width().saturating_sub(viewport_w);
            dirty |= state.scroll_horizontal(delta, max);
        }
        ObjectsMessage::Select => {
            // `Enter` mirrors the original dbm: expand/collapse an expandable
            // row (fetching children on expand), otherwise open an object.
            if let Some(node) = state.toggle_expand() {
                maybe_fetch_on_expand(&node, &state, &mut effects);
                dirty = true;
            } else if let Some(target) = state.selected_target() {
                // Opening an object notifies the SQL workspace; the objects
                // state itself is unchanged (the shell marks the workspace
                // dirty when it opens the object).
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

#[cfg(test)]
mod tests {
    use super::*;

    fn db_row(name: &str) -> super::super::state::ObjectsRow {
        super::super::state::ObjectsRow {
            depth: 0,
            expanded: false,
            expandable: true,
            node: ObjectsNode::Database { name: name.to_string() },
            label: name.to_string(),
        }
    }

    fn obj_row(name: &str) -> super::super::state::ObjectsRow {
        super::super::state::ObjectsRow {
            depth: 1,
            expanded: false,
            expandable: false,
            node: ObjectsNode::Object {
                database: "db".to_string(),
                schema: Some("public".to_string()),
                kind: ObjectKind::Tables,
                name: name.to_string(),
            },
            label: name.to_string(),
        }
    }

    #[test]
    fn select_toggles_expandable_row_and_opens_object() {
        // An expandable database row: Enter toggles expansion (dirty) and,
        // being bound, triggers a schema fetch.
        let mut s = ObjectsState::default();
        s.bound_instance = "inst".to_string();
        s.bound_connection = "conn".to_string();
        s.rows = vec![db_row("db")];
        s.cursor = 0;
        let (s, _i, effects, dirty) = update(ObjectsMessage::Select, s);
        assert!(dirty);
        assert!(s.expanded.contains("db"));
        assert!(
            effects.iter().any(|e| matches!(e, ObjectsEffect::LoadSchemas { .. })),
            "expected LoadSchemas on db expand, got {effects:?}"
        );

        // An object row: Enter opens it (intent, no dirty).
        let mut s = ObjectsState::default();
        s.rows = vec![obj_row("t1")];
        s.cursor = 0;
        let (_, intents, _, dirty) = update(ObjectsMessage::Select, s);
        assert!(!dirty);
        assert!(intents.iter().any(|i| matches!(i, ObjectsIntent::OpenObject { .. })));
    }

    #[test]
    fn collapse_removes_expansion_key() {
        let mut s = ObjectsState::default();
        s.rows = vec![db_row("db")];
        s.expanded.insert("db".to_string());
        s.cursor = 0;

        // Collapse removes the key and reports dirty.
        let (s, _i, _e, dirty) = update(ObjectsMessage::Collapse, s);
        assert!(dirty);
        assert!(!s.expanded.contains("db"));

        // Collapsing when already collapsed is a no-op.
        let (_s, _i, _e, dirty) = update(ObjectsMessage::Collapse, s);
        assert!(!dirty);
    }

    #[test]
    fn horizontal_scroll_clamps_and_reports_noop() {
        let s = ObjectsState::default();
        let (s, _i, _e, dirty) = update(ObjectsMessage::ScrollHorizontal { delta: -1, term_width: 80 }, s);
        assert!(!dirty, "left scroll at boundary is a no-op");
        assert_eq!(s.h_scroll, 0);

        // Empty state has no content → max=0 → scrolling right is also a
        // no-op (content fits fully in viewport).
        let (s, _i, _e, dirty) = update(ObjectsMessage::ScrollHorizontal { delta: 4, term_width: 80 }, s);
        assert!(!dirty, "right scroll on empty state is a no-op");
        assert_eq!(s.h_scroll, 0);
    }
}
