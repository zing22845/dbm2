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
        ObjectsMessage::MoveUp => {
            state.scroll_locked = false;
            dirty |= state.move_up();
        }
        ObjectsMessage::MoveDown => {
            state.scroll_locked = false;
            dirty |= state.move_down();
        }
        ObjectsMessage::JumpTo { row } => {
            state.scroll_locked = false;
            dirty |= state.jump_to(row);
        }
        ObjectsMessage::SetVScroll { position } => {
            let total = state.rows.len();
            let clamped = position.min(total.saturating_sub(1));
            let changed = state.scroll.get() != clamped;
            state.scroll.set(clamped);
            state.scroll_locked = true;
            dirty |= changed;
        }
        ObjectsMessage::SetHScroll { position } => {
            // Clamp upper bound to the viewport-aware max cached by the renderer
            // — same bound used by Paragraph::scroll + h_scrollbar thumb.
            let max = state.cached_h_max_scroll.get();
            let clamped = position.min(max);
            let changed = state.h_scroll as usize != clamped;
            state.h_scroll = clamped.min(u16::MAX as usize) as u16;
            dirty |= changed;
        }
        ObjectsMessage::Collapse => {
            dirty |= state.collapse();
        }
        ObjectsMessage::ScrollHorizontal {
            delta,
            term_width: _,
        } => {
            // Use the viewport-aware max cached by the renderer — this is the
            // same bound used by Paragraph::scroll + h_scrollbar thumb, so an
            // already-at-boundary press is a pure no-op.
            let max = state.cached_h_max_scroll.get().min(u16::MAX as usize) as u16;
            dirty |= state.scroll_horizontal(delta, max);
        }
        ObjectsMessage::Select => {
            // `Enter` mirrors the original dbm: activating a schema row sets it
            // as the active schema (highlighted + forced expanded); database /
            // group rows toggle expand/collapse (fetching children on expand);
            // a table row opens its data view.
            // Clone the schema row's identity before mutating state (the cursor
            // node is a borrow of `state`).
            let schema_active = match state.node_at_cursor() {
                Some(ObjectsNode::Schema { database, name }) => {
                    Some((database.clone(), name.clone()))
                }
                _ => None,
            };
            if let Some((database, name)) = schema_active {
                state.set_active(Some(database.clone()), Some(name.clone()));
                intents.push(ObjectsIntent::ApplySchema { database, name });
                dirty = true;
            } else {
                if let Some(node) = state.toggle_expand() {
                    maybe_fetch_on_expand(&node, &state, &mut effects);
                    dirty = true;
                } else if let Some(target) = state.selected_target() {
                    intents.push(ObjectsIntent::OpenObject { target });
                }
            }
        }
        ObjectsMessage::ToggleExpandAt { row } => {
            // A mouse marker click toggles the database/group at that visible
            // row without moving the cursor (and without activating a schema or
            // opening an object). Fetch children on a fresh expand.
            if let Some(node) = state.toggle_expand_at(row) {
                maybe_fetch_on_expand(&node, &state, &mut effects);
                dirty = true;
            }
        }
        ObjectsMessage::Expand => {
            // `l` expands the cursor's row (database, schema, or group) and only
            // expands — it never collapses (`h` collapses), matching the instances
            // pane. Unlike `Select`, it never activates a schema or opens an
            // object. Object (leaf) rows and already-expanded rows are no-ops.
            if let Some(node) = state.expand() {
                maybe_fetch_on_expand(&node, &state, &mut effects);
                dirty = true;
            }
        }
        ObjectsMessage::Bind {
            instance,
            connection,
        } => {
            // Idempotent: only rebind (and re-fetch databases) when the binding
            // actually changes. The shell's binding sync may emit duplicate
            // `Bind` messages before the first one is applied; an unconditional
            // `rebind` would reset the catalog and re-fetch databases every
            // time, so a no-change bind is skipped.
            let changed = state.sync_binding(instance.clone(), connection.clone());
            dirty = changed;
            if changed {
                effects.push(ObjectsEffect::LoadDatabases {
                    instance,
                    connection,
                });
            }
        }
        ObjectsMessage::DatabasesLoaded { databases } => {
            state.catalog.databases = CatalogList::Ready(databases);
            dirty = state.rebuild_rows();
            // The active database (restored from the active SQL tab's schema) is
            // forced expanded, so fetch its schemas just like a manually-
            // expanded database. Without this, re-binding to a connection shows
            // only the Extensions group and never the schemas.
            if !state.bound_instance.is_empty()
                && !state.bound_connection.is_empty()
                && let Some(db) = state.active_db.clone()
                && !state.catalog.schemas.contains_key(&db)
            {
                effects.push(ObjectsEffect::LoadSchemas {
                    instance: state.bound_instance.clone(),
                    connection: state.bound_connection.clone(),
                    database: db,
                });
            }
        }
        ObjectsMessage::DatabasesError { error } => {
            state.catalog.databases = CatalogList::Error(error);
            dirty = state.rebuild_rows();
        }
        ObjectsMessage::SchemasLoaded { database, schemas } => {
            state
                .catalog
                .schemas
                .insert(database, CatalogList::Ready(schemas));
            // A deferred active schema (from session restore or connection
            // activation) can now be validated: mark it active if present,
            // otherwise degrade to no active schema.
            state.resolve_pending_active_schema();
            dirty = state.rebuild_rows();
        }
        ObjectsMessage::SchemasError { database, error } => {
            state
                .catalog
                .schemas
                .insert(database, CatalogList::Error(error));
            dirty = state.rebuild_rows();
        }
        ObjectsMessage::ExtensionsLoaded {
            database,
            extensions,
        } => {
            state
                .catalog
                .extensions
                .insert(database, CatalogList::Ready(extensions));
            dirty = state.rebuild_rows();
        }
        ObjectsMessage::ExtensionsError { database, error } => {
            state
                .catalog
                .extensions
                .insert(database, CatalogList::Error(error));
            dirty = state.rebuild_rows();
        }
        ObjectsMessage::ObjectListLoaded {
            database,
            schema,
            kind,
            items,
        } => {
            state
                .catalog
                .objects
                .insert((database, schema, kind), CatalogList::Ready(items));
            dirty = state.rebuild_rows();
        }
        ObjectsMessage::ObjectListError {
            database,
            schema,
            kind,
            error,
        } => {
            state
                .catalog
                .objects
                .insert((database, schema, kind), CatalogList::Error(error));
            dirty = state.rebuild_rows();
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
        ObjectsNode::Group {
            database,
            schema,
            kind,
        } => match kind {
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
            active: false,
            node: ObjectsNode::Database {
                name: name.to_string(),
            },
            label: name.to_string(),
        }
    }

    fn obj_row(name: &str) -> super::super::state::ObjectsRow {
        super::super::state::ObjectsRow {
            depth: 1,
            expanded: false,
            expandable: false,
            active: false,
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
    fn select_on_schema_activates_instead_of_toggling() {
        // Enter on a schema row activates it (sets active_db/active_schema and
        // emits ApplySchema) rather than toggling expansion, matching the
        // original dbm.
        let mut s = ObjectsState::default();
        s.rows = vec![super::super::state::ObjectsRow {
            depth: 1,
            expanded: false,
            expandable: true,
            active: false,
            node: ObjectsNode::Schema {
                database: "db".to_string(),
                name: "public".to_string(),
            },
            label: "public".to_string(),
        }];
        s.cursor = 0;
        let (s, intents, _effects, dirty) = update(ObjectsMessage::Select, s);
        assert!(dirty);
        assert_eq!(s.active_db.as_deref(), Some("db"));
        assert_eq!(s.active_schema.as_deref(), Some("public"));
        // It must NOT toggle-expand (expansion is derived from the active path).
        assert!(intents.iter().any(|i| matches!(
            i,
            ObjectsIntent::ApplySchema { database, name }
                if database == "db" && name == "public"
        )));
    }

    #[test]
    fn expand_on_a_schema_row_expands_without_activating_and_never_collapses() {
        // `l` on a schema row expands it (showing the group headers) without
        // activating it (no ApplySchema, no active change), unlike `Select`/Enter.
        let mut s = ObjectsState::default();
        s.rows = vec![super::super::state::ObjectsRow {
            depth: 1,
            expanded: false,
            expandable: true,
            active: false,
            node: ObjectsNode::Schema {
                database: "db".to_string(),
                name: "public".to_string(),
            },
            label: "public".to_string(),
        }];
        s.cursor = 0;
        let (s, intents, _effects, dirty) = update(ObjectsMessage::Expand, s);
        assert!(dirty);
        assert!(s.expanded.contains("db\tpublic"), "schema row is expanded");
        assert!(
            !intents
                .iter()
                .any(|i| matches!(i, ObjectsIntent::ApplySchema { .. })),
            "Expand must not activate the schema"
        );
        assert_eq!(s.active_db, None, "active state is untouched");

        // `l` on an already-expanded row is a no-op (it never collapses; `h` does).
        let (s, _i, _e, dirty) = update(ObjectsMessage::Expand, s);
        assert!(!dirty, "re-expanding an expanded row is a no-op");
        assert!(s.expanded.contains("db\tpublic"));
    }

    #[test]
    fn databases_loaded_fetches_schemas_for_the_active_database() {
        // After re-binding to a connection, `DatabasesLoaded` arrives while the
        // active schema (restored from the SQL tab) points at a database whose
        // schemas are not cached. The active database is force-expanded, so it
        // must fetch its schemas — otherwise only the Extensions group shows.
        let mut s = ObjectsState::default();
        s.bound_instance = "inst".into();
        s.bound_connection = "c1".into();
        s.active_db = Some("postgres".into());
        s.active_schema = Some("public".into());
        let (_s, _intents, effects, _dirty) = update(
            ObjectsMessage::DatabasesLoaded {
                databases: vec!["postgres".into()],
            },
            s,
        );
        assert!(
            effects.iter().any(|e| matches!(
                e,
                ObjectsEffect::LoadSchemas {
                    instance,
                    connection,
                    database,
                } if instance == "inst" && connection == "c1" && database == "postgres"
            )),
            "expected LoadSchemas for the active database, got {effects:?}"
        );
    }

    #[test]
    fn schemas_loaded_marks_or_degrades_a_deferred_active_schema() {
        // Session restore / connection activation defers the active schema until
        // its database's schemas load. `SchemasLoaded` must mark it active when
        // present, and degrade to no active schema when it no longer exists.
        let mut s = ObjectsState::default();
        s.bound_instance = "inst".into();
        s.bound_connection = "c1".into();
        s.active_db = Some("postgres".into());
        s.pending_active_schema = Some(("postgres".into(), "public".into()));

        let (s, _i, _e, _d) = update(
            ObjectsMessage::SchemasLoaded {
                database: "postgres".into(),
                schemas: vec!["public".into(), "extensions".into()],
            },
            s,
        );
        assert_eq!(s.active_schema.as_deref(), Some("public"));

        // A deferred schema that is not in the loaded list degrades to no
        // active schema (the database stays active).
        let mut s2 = ObjectsState::default();
        s2.active_db = Some("postgres".into());
        s2.pending_active_schema = Some(("postgres".into(), "dropped".into()));
        let (s2, _i, _e, _d) = update(
            ObjectsMessage::SchemasLoaded {
                database: "postgres".into(),
                schemas: vec!["public".into()],
            },
            s2,
        );
        assert_eq!(s2.active_db.as_deref(), Some("postgres"));
        assert_eq!(s2.active_schema, None);
    }

    #[test]
    fn toggle_expand_at_toggles_that_row_without_moving_cursor() {
        // A database row at row 1; cursor is on row 0.
        let mut s = ObjectsState::default();
        s.rows = vec![
            super::super::state::ObjectsRow {
                depth: 0,
                expanded: false,
                expandable: false,
                active: false,
                node: ObjectsNode::Database { name: "db".into() },
                label: "db".into(),
            },
            super::super::state::ObjectsRow {
                depth: 1,
                expanded: false,
                expandable: true,
                active: false,
                node: ObjectsNode::Group {
                    database: "db".into(),
                    schema: Some("public".into()),
                    kind: ObjectKind::Tables,
                },
                label: "Tables".into(),
            },
        ];
        s.cursor = 0;
        // Capture the group's expand key before the update (rebuild_rows inside
        // toggle_expand_at regenerates rows from the catalog).
        let group_key = s.expand_key_of(&s.rows[1].node);
        let (s, _intents, _effects, dirty) = update(ObjectsMessage::ToggleExpandAt { row: 1 }, s);
        assert!(dirty);
        assert!(
            s.expanded.contains(&group_key),
            "the clicked group is toggled expanded"
        );
        assert_eq!(s.cursor, 0, "marker click must not move the cursor");
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
            effects
                .iter()
                .any(|e| matches!(e, ObjectsEffect::LoadSchemas { .. })),
            "expected LoadSchemas on db expand, got {effects:?}"
        );

        // An object row: Enter opens it (intent, no dirty).
        let mut s = ObjectsState::default();
        s.rows = vec![obj_row("t1")];
        s.cursor = 0;
        let (_, intents, _, dirty) = update(ObjectsMessage::Select, s);
        assert!(!dirty);
        assert!(
            intents
                .iter()
                .any(|i| matches!(i, ObjectsIntent::OpenObject { .. }))
        );
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
        let (s, _i, _e, dirty) = update(
            ObjectsMessage::ScrollHorizontal {
                delta: -1,
                term_width: 80,
            },
            s,
        );
        assert!(!dirty, "left scroll at boundary is a no-op");
        assert_eq!(s.h_scroll, 0);

        // Empty state has no content → max=0 → scrolling right is also a
        // no-op (content fits fully in viewport).
        let (s, _i, _e, dirty) = update(
            ObjectsMessage::ScrollHorizontal {
                delta: 4,
                term_width: 80,
            },
            s,
        );
        assert!(!dirty, "right scroll on empty state is a no-op");
        assert_eq!(s.h_scroll, 0);
    }
}
