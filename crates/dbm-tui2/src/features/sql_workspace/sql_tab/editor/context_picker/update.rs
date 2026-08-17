//! Context picker sub-module update.
//!
//! Pure by-value transition. Cursor movement, column switching and `/` search
//! are pure state changes; catalog loads and the applied selection become
//! effects / intents that the parent resolves.

use crate::common::components::search::PaneSearchInput;

use super::msg::ContextPickerMessage;
use super::state::{
    CachedList, ContextPickerState, PickerColumn, clamp_cursor, cursor_for_name, filter_indices,
    item_at_filtered,
};
use super::intent::ContextPickerIntent;
use super::effect::ContextPickerEffect;

/// Update the context picker state. Pure by-value transition.
///
/// The returned `bool` is `dirty`: whether the rendered picker changed. A
/// closed picker ignores everything but `Open` (reporting `false`).
pub fn update(
    msg: ContextPickerMessage,
    mut state: ContextPickerState,
) -> (ContextPickerState, Vec<ContextPickerIntent>, Vec<ContextPickerEffect>, bool) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();

    if !state.open {
        // A picker that is closed only accepts the Open message (everything
        // else is a no-op, e.g. late async results for a closed picker).
        if let ContextPickerMessage::Open { column, instance, connection, database } = msg {
            state = ContextPickerState::open(column, instance, connection, database);
            effects.push(ContextPickerEffect::LoadDatabases {
                instance: state.instance.clone(),
                connection: state.connection.clone(),
            });
            effects.push(ContextPickerEffect::LoadSchemas {
                instance: state.instance.clone(),
                connection: state.connection.clone(),
                database: state.preview_database.clone(),
            });
            return (state, intents, effects, true);
        }
        return (state, intents, effects, false);
    }

    let dirty = match msg {
        ContextPickerMessage::Open { .. } => {
            // Already open; ignore a second open.
            false
        }
        ContextPickerMessage::Close => {
            state.close();
            true
        }
        ContextPickerMessage::MoveCursor { delta } => {
            move_cursor(&mut state, delta, &mut effects);
            true
        }
        ContextPickerMessage::SetCursor { column, cursor } => {
            state.switch_column(column);
            match column {
                PickerColumn::Database => state.db_cursor = cursor,
                PickerColumn::Schema => state.schema_cursor = cursor,
            }
            sync_picker_cursors(&mut state, &mut effects);
            true
        }
        ContextPickerMessage::MoveColumn(column) => {
            let changed = state.switch_column(column);
            if changed {
                sync_picker_cursors(&mut state, &mut effects);
            }
            changed
        }
        ContextPickerMessage::BeginSearch => {
            state.begin_search_input();
            true
        }
        ContextPickerMessage::SearchKey(key) => {
            handle_search_key(&mut state, key, &mut effects);
            true
        }
        ContextPickerMessage::Apply => {
            if let Some((database, schema)) = selected_context(&state) {
                intents.push(ContextPickerIntent::ApplyContext { database, schema });
            }
            state.close();
            true
        }
        ContextPickerMessage::DatabasesLoaded { items } => {
            // Seed the cursor onto the initial preview database so the picker
            // opens on the tab's active database rather than the first entry.
            state.db_cursor = cursor_for_name(&items, &state.db_search, &state.preview_database);
            state.databases = CachedList::Ready(items);
            sync_picker_cursors(&mut state, &mut effects);
            true
        }
        ContextPickerMessage::DatabasesError { error } => {
            state.databases = CachedList::Error(error);
            true
        }
        ContextPickerMessage::SchemasLoaded { items } => {
            state.schemas = CachedList::Ready(items);
            sync_picker_cursors(&mut state, &mut effects);
            true
        }
        ContextPickerMessage::SchemasError { error } => {
            state.schemas = CachedList::Error(error);
            true
        }
    };

    (state, intents, effects, dirty)
}

/// Move the cursor in the active column and re-sync the preview/cursors.
fn move_cursor(state: &mut ContextPickerState, delta: i32, effects: &mut Vec<ContextPickerEffect>) {
    match state.column {
        PickerColumn::Database => {
            if let CachedList::Ready(items) = &state.databases {
                let filtered = filter_indices(items, &state.db_search);
                let next = if delta > 0 {
                    state.db_cursor.saturating_add(1)
                } else {
                    state.db_cursor.saturating_sub(1)
                };
                state.db_cursor = clamp_cursor(next, filtered.len());
            }
        }
        PickerColumn::Schema => {
            if let CachedList::Ready(items) = &state.schemas {
                let filtered = filter_indices(items, &state.schema_search);
                let next = if delta > 0 {
                    state.schema_cursor.saturating_add(1)
                } else {
                    state.schema_cursor.saturating_sub(1)
                };
                state.schema_cursor = clamp_cursor(next, filtered.len());
            }
        }
    }
    sync_picker_cursors(state, effects);
}

/// Reconcile the preview database with the database cursor, clamp both cursors
/// to their filtered lists, and (re)load schemas when the preview database
/// changes.
fn sync_picker_cursors(state: &mut ContextPickerState, effects: &mut Vec<ContextPickerEffect>) {
    let mut new_schema_cursor = state.schema_cursor;
    if let CachedList::Ready(items) = &state.databases {
        let filtered = filter_indices(items, &state.db_search);
        state.db_cursor = clamp_cursor(state.db_cursor, filtered.len());
        let preview_changed = filtered
            .get(state.db_cursor)
            .map(|&i| items[i].clone())
            .filter(|name| *name != state.preview_database);
        if let Some(new_preview) = preview_changed {
            state.preview_database = new_preview;
            state.schemas = CachedList::Loading;
            state.schema_cursor = 0;
            effects.push(ContextPickerEffect::LoadSchemas {
                instance: state.instance.clone(),
                connection: state.connection.clone(),
                database: state.preview_database.clone(),
            });
            return;
        }
    }
    if let CachedList::Ready(items) = &state.schemas {
        let filtered = filter_indices(items, &state.schema_search);
        new_schema_cursor = clamp_cursor(new_schema_cursor, filtered.len());
    }
    state.schema_cursor = new_schema_cursor;
}

/// Handle a key while the active column's search input is live.
fn handle_search_key(
    state: &mut ContextPickerState,
    key: crossterm::event::KeyEvent,
    effects: &mut Vec<ContextPickerEffect>,
) {
    let caps_lock = false;
    let action = match key.code {
        crossterm::event::KeyCode::Esc => {
            state.cancel_search_input();
            PaneSearchInput::Cancelled
        }
        crossterm::event::KeyCode::Enter => {
            state.active_search_mut().end();
            PaneSearchInput::Applied
        }
        _ => state.active_search_mut().handle_key(&key, caps_lock),
    };

    if let PaneSearchInput::Navigate { forward } = action {
        move_cursor(state, if forward { 1 } else { -1 }, effects);
        return;
    }

    if matches!(
        action,
        PaneSearchInput::QueryChanged | PaneSearchInput::OptionsChanged
    ) {
        match state.column {
            PickerColumn::Database => state.db_cursor = 0,
            PickerColumn::Schema => state.schema_cursor = 0,
        }
    }

    sync_picker_cursors(state, effects);
}

/// The currently selected (database, schema), if both are resolvable.
fn selected_context(state: &ContextPickerState) -> Option<(String, String)> {
    let database = match &state.databases {
        CachedList::Ready(items) => item_at_filtered(items, &state.db_search, state.db_cursor)?,
        _ => return None,
    };
    let schema = match &state.schemas {
        CachedList::Ready(items) => {
            item_at_filtered(items, &state.schema_search, state.schema_cursor)
                .unwrap_or_else(|| super::state::default_schema(items))
        }
        _ => return None,
    };
    Some((database, schema))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, KeyEventKind, KeyEventState};

    fn char_key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn open_emits_both_catalog_loads() {
        let (s, _i, e, _d) = update(
            ContextPickerMessage::Open {
                column: PickerColumn::Database,
                instance: "inst".into(),
                connection: "conn".into(),
                database: "postgres".into(),
            },
            ContextPickerState::default(),
        );
        assert!(s.open);
        assert_eq!(s.preview_database, "postgres");
        assert_eq!(e.len(), 2);
    }

    #[test]
    fn closed_picker_ignores_non_open_messages() {
        let (s, _i, e, _d) = update(ContextPickerMessage::Close, ContextPickerState::default());
        assert!(!s.open);
        assert!(e.is_empty());
    }

    #[test]
    fn databases_loaded_seeds_cursor_onto_active_database() {
        let (mut s, _i, open_effects, _d) = update(
            ContextPickerMessage::Open {
                column: PickerColumn::Database,
                instance: "inst".into(),
                connection: "conn".into(),
                database: "postgres".into(),
            },
            ContextPickerState::default(),
        );
        // Opening already schedules a schema load for the active database.
        assert!(open_effects.iter().any(|eff| matches!(
            eff,
            ContextPickerEffect::LoadSchemas { database, .. } if database == "postgres"
        )));

        // When the databases list arrives, the cursor lands on the active
        // database (postgres, index 1) rather than the first entry.
        let (s2, _i, e2, _d) = update(
            ContextPickerMessage::DatabasesLoaded {
                items: vec!["app".into(), "postgres".into()],
            },
            s,
        );
        s = s2;
        assert!(matches!(s.databases, CachedList::Ready(_)));
        assert_eq!(s.preview_database, "postgres");
        assert_eq!(s.db_cursor, 1);
        // Preview did not change (already postgres), so no extra schema load.
        assert!(e2.is_empty());
    }

    #[test]
    fn apply_emits_intent_and_closes() {
        let (mut s, _i, _e, _d) = update(
            ContextPickerMessage::Open {
                column: PickerColumn::Database,
                instance: "inst".into(),
                connection: "conn".into(),
                database: "postgres".into(),
            },
            ContextPickerState::default(),
        );
        s.databases = CachedList::Ready(vec!["app".into(), "postgres".into()]);
        s.db_cursor = 1; // postgres
        s.schemas = CachedList::Ready(vec!["public".into(), "analytics".into()]);
        s.schema_cursor = 0; // public
        let (s2, i, _e, _d) = update(ContextPickerMessage::Apply, s);
        assert!(!s2.open);
        assert_eq!(i.len(), 1);
        match &i[0] {
            ContextPickerIntent::ApplyContext { database, schema } => {
                assert_eq!(database, "postgres");
                assert_eq!(schema, "public");
            }
        }
    }

    #[test]
    fn move_column_switches_focus() {
        let (mut s, _i, _e, _d) = update(
            ContextPickerMessage::Open {
                column: PickerColumn::Database,
                instance: "inst".into(),
                connection: "conn".into(),
                database: "postgres".into(),
            },
            ContextPickerState::default(),
        );
        let (s2, _i, _e, _d) = update(ContextPickerMessage::MoveColumn(PickerColumn::Schema), s);
        s = s2;
        assert_eq!(s.column, PickerColumn::Schema);
    }

    #[test]
    fn set_cursor_jumps_column_and_focus() {
        let (mut s, _i, _e, _d) = update(
            ContextPickerMessage::Open {
                column: PickerColumn::Database,
                instance: "inst".into(),
                connection: "conn".into(),
                database: "postgres".into(),
            },
            ContextPickerState::default(),
        );
        s.databases = CachedList::Ready(vec!["app".into(), "postgres".into(), "other".into()]);
        s.schemas = CachedList::Ready(vec!["public".into()]);
        let (s2, _i, _e, _d) = update(
            ContextPickerMessage::SetCursor {
                column: PickerColumn::Schema,
                cursor: 0,
            },
            s,
        );
        assert_eq!(s2.column, PickerColumn::Schema);
        assert_eq!(s2.schema_cursor, 0);
    }

    #[test]
    fn search_key_queries_and_resets_cursor() {
        let (mut s, _i, _e, _d) = update(
            ContextPickerMessage::Open {
                column: PickerColumn::Database,
                instance: "inst".into(),
                connection: "conn".into(),
                database: "postgres".into(),
            },
            ContextPickerState::default(),
        );
        s.databases = CachedList::Ready(vec!["app".into(), "postgres".into()]);
        s.db_cursor = 1;
        let (s2, _i, _e, _d) = update(ContextPickerMessage::BeginSearch, s);
        let (s2, _i, _e, _d) = update(ContextPickerMessage::SearchKey(char_key('a')), s2);
        assert!(s2.db_search.active);
        assert_eq!(s2.db_search.query, "a");
        assert_eq!(s2.db_cursor, 0);
        let _ = _e;
    }
}
