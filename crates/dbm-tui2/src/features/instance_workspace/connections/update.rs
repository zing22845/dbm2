//! Instance connections feature update.

use dbm_store::NewInstanceConnection;

use super::effect::ConnectionsEffect;
use super::intent::ConnectionsIntent;
use super::msg::ConnectionsMessage;
use super::state::{ConnectionStatusKind, ConnectionsState, FormField, FormMode};

/// Update the connections panel state. Pure by-value transition.
///
/// The returned `bool` is `dirty`: whether the rendered panel changed.
/// Load-only and delete messages change nothing locally (the later `Loaded`
/// repaint); form/navigation messages report per actual change.
pub fn update(
    msg: ConnectionsMessage,
    mut state: ConnectionsState,
) -> (
    ConnectionsState,
    Vec<ConnectionsIntent>,
    Vec<ConnectionsEffect>,
    bool,
) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    let dirty = match msg {
        ConnectionsMessage::Load { instance_name } => {
            // Remember the bound instance so a later Reload (after add/edit/
            // delete) re-queries the same instance instead of an empty name.
            state.instance_name = instance_name.clone();
            effects.push(ConnectionsEffect::LoadConnections { instance_name });
            false
        }
        ConnectionsMessage::Reload => {
            let instance_name = state.instance_name.clone();
            effects.push(ConnectionsEffect::LoadConnections { instance_name });
            false
        }
        ConnectionsMessage::Loaded { connections } => {
            // Preserve the cursor across a reload by re-resolving the currently
            // selected connection by name. A refresh (e.g. after testing a saved
            // connection) must keep the selection on the row the user was on,
            // not snap back to the first connection. On an initial load (no
            // prior selection) the cursor stays clamped where it was; when the
            // old selection no longer exists (deleted) it clamps into the new
            // list.
            let list_changed = state.connections != connections;
            let prev_name = state.connections.get(state.cursor).map(|c| c.name.clone());
            let old_cursor = state.cursor;
            state.connections = connections;
            let clamp = |c: usize| c.min(state.connections.len().saturating_sub(1));
            let new_cursor = if let Some(saved) = state.restore_cursor.take() {
                // A session-restore cursor is pending: it was saved while the
                // list was still empty (connections load lazily), so apply it
                // now that the rows are present, clamped into range.
                clamp(saved)
            } else {
                match prev_name {
                    // Initial load (or an empty previous list): keep the cursor
                    // where it was, clamped.
                    None => clamp(old_cursor),
                    Some(name) => state
                        .connections
                        .iter()
                        .position(|c| c.name == name)
                        .unwrap_or_else(|| clamp(old_cursor)),
                }
            };
            let cursor_changed = new_cursor != old_cursor;
            state.cursor = new_cursor;
            // Repaint only when the list (or the cursor) actually changed, so a
            // refresh that reloads identical data does not redraw.
            list_changed || cursor_changed
        }
        ConnectionsMessage::MoveUp => {
            let dirty = state.move_up();
            if dirty {
                state.scroll_locked = false;
            }
            dirty
        }
        ConnectionsMessage::MoveDown => {
            let dirty = state.move_down();
            if dirty {
                state.scroll_locked = false;
            }
            dirty
        }
        ConnectionsMessage::JumpTo { row } => {
            if state.connections.is_empty() {
                false
            } else {
                let clamped = row.min(state.connections.len() - 1);
                let changed = state.cursor != clamped;
                state.cursor = clamped;
                // A click targets a visible row, so unlock the anchor so the
                // viewport stops fighting the manual jump on the next frame.
                state.scroll_locked = false;
                changed
            }
        }
        ConnectionsMessage::BeginAdd => {
            let changed = state.begin_add();
            if changed {
                // Opening the form clears any previous test status.
                state.status = None;
                state.status_kind = ConnectionStatusKind::Idle;
            }
            changed
        }
        ConnectionsMessage::BeginEdit => {
            let changed = state.begin_edit(state.cursor);
            if changed {
                state.status = None;
                state.status_kind = ConnectionStatusKind::Idle;
            }
            changed
        }
        ConnectionsMessage::CancelForm => {
            let changed = state.form.is_some();
            state.form = None;
            if changed {
                state.status = None;
                state.status_kind = ConnectionStatusKind::Idle;
            }
            changed
        }
        ConnectionsMessage::CommitForm => {
            // Saving the whole connection only happens in normal mode; in insert
            // mode `Enter` is handled by CommitFieldInsert instead. Guard here so
            // a stray CommitForm in insert mode does not save prematurely.
            if state
                .form
                .as_ref()
                .is_some_and(|f| f.mode == FormMode::Insert)
            {
                return (state, intents, effects, false);
            }
            // Keep the form open until the save actually succeeds: it is closed
            // on `Saved` and the error is surfaced on `Error` (fixing a failed
            // save silently dropping the form and leaving an empty list/table).
            let Some(form) = state.form.as_ref() else {
                return (state, intents, effects, false);
            };
            if form.name.trim().is_empty() {
                // Name is required; keep the form open for correction.
                return (state, intents, effects, false);
            }
            let connection = NewInstanceConnection {
                name: form.name.trim().to_string(),
                username: form.username.clone(),
                database: form.database.clone(),
                password: if form.password.is_empty() {
                    None
                } else {
                    Some(form.password.clone())
                },
                ssl_mode: Some(form.ssl_mode.clone()),
                env_label: None,
            };
            let instance_name = state.instance_name.clone();
            match form.edit_original_name.as_ref() {
                Some(original_name) => effects.push(ConnectionsEffect::EditConnection {
                    instance_name,
                    original_name: original_name.clone(),
                    connection,
                }),
                None => effects.push(ConnectionsEffect::AddConnection {
                    instance_name,
                    connection,
                }),
            }
            true
        }
        ConnectionsMessage::Saved => {
            // The add/edit succeeded: close the form and reload the list so the
            // new/updated row appears (and the connection is visible in both the
            // list and the store).
            state.form = None;
            let instance_name = state.instance_name.clone();
            effects.push(ConnectionsEffect::LoadConnections {
                instance_name: instance_name.clone(),
            });
            // Notify the shell so the explorer tree for this instance refreshes
            // and shows the change immediately (matching the original dbm's
            // `load_instance_connections` on save).
            intents.push(ConnectionsIntent::ConnectionsChanged { instance_name });
            state.status = None;
            state.status_kind = ConnectionStatusKind::Idle;
            true
        }
        ConnectionsMessage::SaveError(error) => {
            // Keep the form open so the user can fix the fields, and show the
            // error on the footer instead of silently dropping the form.
            let changed = state.status.as_deref() != Some(error.as_str());
            state.status = Some(error);
            state.status_kind = ConnectionStatusKind::Failure;
            changed
        }
        ConnectionsMessage::SetStatus { status, kind } => {
            let changed = state.status.as_deref() != Some(status.as_str());
            state.status = Some(status);
            state.status_kind = kind;
            changed
        }
        ConnectionsMessage::TestComplete { ok, error } => {
            // Show the list-test result (with a timestamp) and reload so the
            // row's test timestamps and whole-row color update. The status and
            // the reloaded rows are both rendered by the `Loaded` repaint that
            // follows, so do not mark this dirty here: repainting now would
            // only change the footer status while the whole connection list
            // stays identical, i.e. a ~66%-redundant redraw on every test.
            let ts = crate::common::utils::time::utc_timestamp();
            let (status, kind) = if ok {
                (format!("{ts} Test OK"), ConnectionStatusKind::Success)
            } else {
                (
                    format!(
                        "{ts} Test failed: {}",
                        error.unwrap_or_else(|| "could not connect".to_string())
                    ),
                    ConnectionStatusKind::Failure,
                )
            };
            state.status = Some(status);
            state.status_kind = kind;
            let instance_name = state.instance_name.clone();
            effects.push(ConnectionsEffect::LoadConnections { instance_name });
            false
        }
        ConnectionsMessage::TestForm => {
            // Limit tests to once per second (shared with the list `t`).
            state.test_cooldown_until =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(1));
            // Test the form's current values against the database (no save).
            let Some(form) = state.form.as_ref() else {
                return (state, intents, effects, false);
            };
            if form.name.trim().is_empty() {
                return (state, intents, effects, false);
            }
            let instance_name = state.instance_name.clone();
            let connection = NewInstanceConnection {
                name: form.name.trim().to_string(),
                username: form.username.clone(),
                database: form.database.clone(),
                password: if form.password.is_empty() {
                    None
                } else {
                    Some(form.password.clone())
                },
                ssl_mode: Some(form.ssl_mode.clone()),
                env_label: None,
            };
            // When editing an existing connection and the password field is
            // blank (not re-entered), the test must still use the form's current
            // name/username/database and only borrow the stored password — so a
            // modified field value is actually exercised, not the old saved one.
            if connection.password.is_none()
                && let Some(original_name) = form.edit_original_name.as_ref()
            {
                effects.push(ConnectionsEffect::TestEditedFormConnection {
                    instance_name,
                    original_name: original_name.clone(),
                    connection,
                });
            } else {
                effects.push(ConnectionsEffect::TestFormConnection {
                    instance_name,
                    connection,
                });
            }
            false
        }
        ConnectionsMessage::TestSelected => {
            // Limit tests to once per second (shared with the form `t`).
            state.test_cooldown_until =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(1));
            // Test the selected saved connection with its stored credentials.
            let Some(connection_name) = state.selected_name() else {
                return (state, intents, effects, false);
            };
            let instance_name = state.instance_name.clone();
            effects.push(ConnectionsEffect::TestConnection {
                instance_name,
                connection_name,
            });
            // See `TestForm`: starting the async test renders nothing new, so a
            // dirty repaint here would be pure waste on every cooldown expiry
            // while `t` is held.
            false
        }
        ConnectionsMessage::DeleteConnection {
            instance_name,
            connection_name,
        } => {
            if !instance_name.is_empty() && !connection_name.is_empty() {
                effects.push(ConnectionsEffect::DeleteConnection {
                    instance_name: instance_name.clone(),
                    connection_name,
                });
                intents.push(ConnectionsIntent::ConnectionsChanged { instance_name });
            }
            false
        }
        ConnectionsMessage::FormField(field) => {
            // Field navigation only applies in normal mode; while inserting, the
            // cursor stays on the field being edited (matching the original dbm).
            if state
                .form
                .as_ref()
                .is_some_and(|f| f.mode == FormMode::Normal)
            {
                if let Some(form) = state.form.as_mut() {
                    let changed = form.field != field;
                    form.field = field;
                    changed
                } else {
                    false
                }
            } else {
                false
            }
        }
        ConnectionsMessage::CycleSslMode(delta) => {
            // The sslmode selector is a chooser: it only cycles in normal mode
            // while the cursor sits on it.
            let active = state
                .form
                .as_ref()
                .is_some_and(|f| f.mode == FormMode::Normal && f.field == FormField::SslMode);
            if active {
                state.form.as_mut().is_some_and(|f| f.cycle_ssl_mode(delta))
            } else {
                false
            }
        }
        ConnectionsMessage::FormClick { field, is_double } => {
            // Match the original dbm's form field click handling: clicking a
            // field while another is being edited commits that in-progress edit
            // and moves the cursor to the clicked field; a double click then
            // enters insert mode on it. A single click just selects the field.
            if state.form.is_none() {
                return (state, intents, effects, false);
            }
            let was_insert = state.form.as_ref().unwrap().mode == FormMode::Insert;
            let on_other = state.form.as_ref().unwrap().field != field;
            if was_insert && on_other {
                state.commit_field_insert();
            }
            state.form.as_mut().unwrap().field = field;
            if is_double {
                state.begin_field_insert();
            } else if state.form.as_ref().unwrap().mode == FormMode::Insert {
                state.commit_field_insert();
            }
            true
        }
        ConnectionsMessage::BeginFieldInsert => state.begin_field_insert(),
        ConnectionsMessage::CommitFieldInsert => state.commit_field_insert(),
        ConnectionsMessage::CancelFieldInsert => state.cancel_field_insert(),
        ConnectionsMessage::ClearFieldAndInsert => state.clear_field_and_insert(),
        ConnectionsMessage::SetPendingD => state.set_pending_d(),
        ConnectionsMessage::FormChar(c) if !c.is_control() => state.form_insert_char(c),
        ConnectionsMessage::FormChar(_) => false,
        ConnectionsMessage::FormBackspace => state.form_backspace(),
        ConnectionsMessage::SetVScroll { position } => {
            let max = state.connections.len().saturating_sub(1);
            let prev = state.scroll;
            state.scroll = position.min(max);
            state.scroll_locked = true;
            state.scroll != prev
        }
    };
    (state, intents, effects, dirty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::instance_workspace::connections::state::{ConnectionForm, FormField};

    fn mk_conn(name: &str) -> dbm_store::InstanceConnection {
        dbm_store::InstanceConnection {
            id: name.into(),
            instance_id: "inst".into(),
            name: name.into(),
            username: "postgres".into(),
            database: "postgres".into(),
            has_password: false,
            ssl_mode: "prefer".into(),
            env_label: None,
            created_at: "now".into(),
            updated_at: "now".into(),
            test_succeeded_at: None,
            test_failed_at: None,
        }
    }

    #[test]
    fn jump_to_moves_cursor_and_unlocks_scroll() {
        let mut s = ConnectionsState::default();
        s.connections = vec![mk_conn("a"), mk_conn("b"), mk_conn("c")];
        s.cursor = 0;
        s.scroll_locked = true; // a prior manual drag locked the anchor
        let (s, _i, _e, dirty) = update(
            ConnectionsMessage::JumpTo { row: 2 },
            std::mem::take(&mut s),
        );
        assert_eq!(s.cursor, 2);
        assert!(!s.scroll_locked, "click unlocks the discover anchor");
        assert!(dirty);
    }

    #[test]
    fn jump_to_clamps_out_of_range_row_and_is_noop_when_unchanged() {
        let mut s = ConnectionsState::default();
        s.connections = vec![mk_conn("a"), mk_conn("b")];
        s.cursor = 0;
        // Out-of-range row clamps into the list (99 -> last row 1).
        let (s, _i, _e, dirty) = update(
            ConnectionsMessage::JumpTo { row: 99 },
            std::mem::take(&mut s),
        );
        assert_eq!(s.cursor, 1);
        assert!(dirty);
        // Same-row jump is a no-op.
        let s = ConnectionsState { cursor: 1, ..s };
        let (s, _i, _e, dirty) = update(ConnectionsMessage::JumpTo { row: 1 }, s);
        assert_eq!(s.cursor, 1);
        assert!(!dirty);
    }

    #[test]
    fn jump_to_empty_list_is_noop() {
        let mut s = ConnectionsState::default();
        let (s, _i, _e, dirty) = update(
            ConnectionsMessage::JumpTo { row: 3 },
            std::mem::take(&mut s),
        );
        assert_eq!(s.cursor, 0);
        assert!(!dirty);
    }

    #[test]
    fn load_binds_instance_name_for_later_reload() {
        // Regression: after `Load` binds an instance, `Reload` (dispatched
        // after an add/edit/delete) must re-query the same instance rather than
        // an empty name, otherwise the list comes back empty.
        let s = ConnectionsState::default();
        let (s, _i, _effects, _dirty) = update(
            ConnectionsMessage::Load {
                instance_name: "inst".to_string(),
            },
            s,
        );
        assert_eq!(s.instance_name, "inst");
        let (_, _i, reload_effects, _dirty) = update(ConnectionsMessage::Reload, s);
        assert!(
            reload_effects
                .iter()
                .any(|e| matches!(e, ConnectionsEffect::LoadConnections { instance_name } if instance_name == "inst")),
            "Reload must query the bound instance, got {reload_effects:?}"
        );
    }

    #[test]
    fn loaded_identical_connections_does_not_repaint() {
        let conn = dbm_store::InstanceConnection {
            id: "c1".into(),
            instance_id: "inst".into(),
            name: "main".into(),
            username: "postgres".into(),
            database: "postgres".into(),
            has_password: false,
            ssl_mode: "prefer".into(),
            env_label: None,
            created_at: "now".into(),
            updated_at: "now".into(),
            test_succeeded_at: None,
            test_failed_at: None,
        };
        let s = ConnectionsState::default();
        // First load with two connections repaints.
        let (s, _i, _e, dirty) = update(
            ConnectionsMessage::Loaded {
                connections: vec![conn.clone(), conn.clone()],
            },
            s,
        );
        assert!(dirty);
        // Re-loading the identical list (refresh with unchanged data) must NOT
        // repaint — otherwise a held `r` redraws every second.
        let (_s, _i, _e, dirty) = update(
            ConnectionsMessage::Loaded {
                connections: vec![conn.clone(), conn.clone()],
            },
            s,
        );
        assert!(!dirty);
    }

    #[test]
    fn loaded_preserves_cursor_on_the_selected_connection() {
        fn conn(name: &str) -> dbm_store::InstanceConnection {
            dbm_store::InstanceConnection {
                id: name.into(),
                instance_id: "inst".into(),
                name: name.into(),
                username: "postgres".into(),
                database: "postgres".into(),
                has_password: false,
                ssl_mode: "prefer".into(),
                env_label: None,
                created_at: "now".into(),
                updated_at: "now".into(),
                test_succeeded_at: None,
                test_failed_at: None,
            }
        }
        // Three connections, cursor on the middle one ("b").
        let (mut s, _i, _e, _d) = update(
            ConnectionsMessage::Loaded {
                connections: vec![conn("a"), conn("b"), conn("c")],
            },
            ConnectionsState::default(),
        );
        assert_eq!(
            s.cursor, 0,
            "initial load keeps the cursor on the first row"
        );
        s.cursor = 1;

        // Testing "b" reloads the list; the cursor must stay on "b", not reset
        // to the first connection.
        let (s, _i, _e, _d) = update(
            ConnectionsMessage::Loaded {
                connections: vec![conn("a"), conn("b"), conn("c")],
            },
            s,
        );
        assert_eq!(s.cursor, 1, "cursor stays on the tested connection");

        // If the selected connection disappears (deleted), clamp into the list.
        let (s, _i, _e, _d) = update(
            ConnectionsMessage::Loaded {
                connections: vec![conn("a"), conn("c")],
            },
            s,
        );
        assert_eq!(s.cursor, 1, "cursor clamps when the selection is gone");
    }

    #[test]
    fn loaded_applies_a_queued_restore_cursor_then_clears_it() {
        fn conn(name: &str) -> dbm_store::InstanceConnection {
            dbm_store::InstanceConnection {
                id: name.into(),
                instance_id: "inst".into(),
                name: name.into(),
                username: "postgres".into(),
                database: "postgres".into(),
                has_password: false,
                ssl_mode: "prefer".into(),
                env_label: None,
                created_at: "now".into(),
                updated_at: "now".into(),
                test_succeeded_at: None,
                test_failed_at: None,
            }
        }
        // Session restore queues the saved cursor (the list was empty then).
        let s = ConnectionsState {
            restore_cursor: Some(1),
            ..ConnectionsState::default()
        };
        let (mut s, _i, _e, _d) = update(
            ConnectionsMessage::Loaded {
                connections: vec![conn("a"), conn("b"), conn("c")],
            },
            s,
        );
        assert_eq!(s.cursor, 1, "restored cursor is applied to the loaded rows");
        assert_eq!(
            s.restore_cursor, None,
            "restore cursor is consumed after the first load"
        );

        // Once consumed, a later load reuses the normal by-name preservation.
        s.cursor = 2;
        let (s, _i, _e, _d) = update(
            ConnectionsMessage::Loaded {
                connections: vec![conn("a"), conn("b"), conn("c")],
            },
            s,
        );
        assert_eq!(s.cursor, 2, "normal reload keeps the selected connection");
        assert_eq!(s.restore_cursor, None);
    }

    #[test]
    fn saved_closes_form_and_reloads() {
        let mut s = ConnectionsState {
            instance_name: "inst".into(),
            form: Some(ConnectionForm {
                name: "main".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (s, i, effects, dirty) = update(ConnectionsMessage::Saved, std::mem::take(&mut s));
        // The form closes and the list reloads so the new row appears.
        assert!(dirty);
        assert!(s.form.is_none());
        assert!(effects
            .iter()
            .any(|e| matches!(e, ConnectionsEffect::LoadConnections { instance_name } if instance_name == "inst")));
        // The shell is notified so the explorer tree for this instance refreshes.
        assert!(i.iter().any(|it| matches!(
            it,
            ConnectionsIntent::ConnectionsChanged { instance_name } if instance_name == "inst"
        )));
    }

    #[test]
    fn save_error_keeps_form_and_shows_status() {
        let mut s = ConnectionsState {
            instance_name: "inst".into(),
            form: Some(ConnectionForm {
                name: "main".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (s, _i, _e, dirty) = update(
            ConnectionsMessage::SaveError("SELECT 1 failed".into()),
            std::mem::take(&mut s),
        );
        // A failed save keeps the form open for correction and shows the error
        // on the footer (fixes a silent drop that left the list/table empty).
        assert!(dirty);
        assert!(s.form.is_some(), "form must stay open on save error");
        assert_eq!(s.status.as_deref(), Some("SELECT 1 failed"));
    }

    #[test]
    fn field_insert_commits_and_cancels() {
        let mut s = ConnectionsState {
            form: Some(ConnectionForm {
                name: "main".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        // Enter insert mode on Name, snapshotting "main".
        let (mut s, _i, _e, dirty) =
            update(ConnectionsMessage::BeginFieldInsert, std::mem::take(&mut s));
        assert!(dirty);
        assert_eq!(s.form.as_ref().unwrap().mode, FormMode::Insert);
        // Type into the field while in insert mode.
        let (mut s, _i, _e, _) = update(ConnectionsMessage::FormChar('2'), std::mem::take(&mut s));
        assert_eq!(s.form.as_ref().unwrap().name, "main2");
        // Cancel the field edit reverts to the snapshot.
        let (s, _i, _e, dirty) = update(
            ConnectionsMessage::CancelFieldInsert,
            std::mem::take(&mut s),
        );
        assert!(dirty);
        assert_eq!(s.form.as_ref().unwrap().mode, FormMode::Normal);
        assert_eq!(s.form.as_ref().unwrap().name, "main");
    }

    #[test]
    fn form_click_single_selects_field() {
        // A single click on a field row moves the field cursor to it and stays
        // in normal mode.
        let mut s = ConnectionsState {
            form: Some(ConnectionForm {
                name: "a".into(),
                field: FormField::Name,
                mode: FormMode::Normal,
                ..Default::default()
            }),
            ..Default::default()
        };
        let (s, _i, _e, dirty) = update(
            ConnectionsMessage::FormClick {
                field: FormField::Database,
                is_double: false,
            },
            std::mem::take(&mut s),
        );
        assert!(dirty);
        let f = s.form.expect("form stays open on a click");
        assert_eq!(
            f.field,
            FormField::Database,
            "single click selects the clicked field"
        );
        assert_eq!(f.mode, FormMode::Normal, "single click keeps normal mode");
    }

    #[test]
    fn form_click_double_enters_insert_mode() {
        // A double click on a field row selects it and enters insert mode.
        let mut s = ConnectionsState {
            form: Some(ConnectionForm {
                name: "a".into(),
                field: FormField::Name,
                mode: FormMode::Normal,
                ..Default::default()
            }),
            ..Default::default()
        };
        let (s, _i, _e, dirty) = update(
            ConnectionsMessage::FormClick {
                field: FormField::Password,
                is_double: true,
            },
            std::mem::take(&mut s),
        );
        assert!(dirty);
        let f = s.form.expect("form stays open on a double click");
        assert_eq!(
            f.field,
            FormField::Password,
            "double click moves to the clicked field"
        );
        assert_eq!(f.mode, FormMode::Insert, "double click enters insert mode");
    }

    #[test]
    fn form_click_moving_fields_commits_in_progress_edit() {
        // Editing Name; a single click on Database commits the Name edit (keeps
        // its typed value) and moves the cursor to Database in normal mode.
        let mut s = ConnectionsState {
            form: Some(ConnectionForm {
                name: "a".into(),
                field: FormField::Name,
                mode: FormMode::Insert,
                insert_field_snapshot: Some("a".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (s, _i, _e, dirty) = update(
            ConnectionsMessage::FormClick {
                field: FormField::Database,
                is_double: false,
            },
            std::mem::take(&mut s),
        );
        assert!(dirty);
        let f = s.form.expect("form stays open");
        assert_eq!(f.field, FormField::Database);
        assert_eq!(
            f.mode,
            FormMode::Normal,
            "moving fields commits the in-progress edit"
        );
        assert_eq!(f.name, "a", "the committed edit keeps its typed value");
    }

    #[test]
    fn form_click_same_field_in_insert_commits_it() {
        // A single click on the field currently being edited commits the edit
        // and returns to normal mode (matching the original dbm).
        let mut s = ConnectionsState {
            form: Some(ConnectionForm {
                name: "abc".into(),
                field: FormField::Name,
                mode: FormMode::Insert,
                insert_field_snapshot: Some("a".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (s, _i, _e, dirty) = update(
            ConnectionsMessage::FormClick {
                field: FormField::Name,
                is_double: false,
            },
            std::mem::take(&mut s),
        );
        assert!(dirty);
        let f = s.form.unwrap();
        assert_eq!(
            f.mode,
            FormMode::Normal,
            "clicking the field being edited commits it"
        );
    }

    #[test]
    fn form_click_ignored_when_form_closed() {
        let mut s = ConnectionsState::default();
        let (_s, _i, effects, dirty) = update(
            ConnectionsMessage::FormClick {
                field: FormField::Name,
                is_double: false,
            },
            std::mem::take(&mut s),
        );
        assert!(!dirty);
        assert!(effects.is_empty());
    }

    #[test]
    fn test_form_emits_test_effect() {
        let mut s = ConnectionsState {
            instance_name: "inst".into(),
            form: Some(ConnectionForm {
                name: "main".into(),
                username: "postgres".into(),
                database: "appdb".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (s, _i, effects, dirty) = update(ConnectionsMessage::TestForm, std::mem::take(&mut s));
        // Starting the async test renders nothing new, so it must not mark the
        // round dirty (a redundant repaint while `t` is held).
        assert!(
            !dirty,
            "starting a form test must not trigger a redundant repaint"
        );
        assert!(effects.iter().any(|e| matches!(
            e,
            ConnectionsEffect::TestFormConnection { instance_name, connection }
                if instance_name == "inst" && connection.name == "main"
        )));
        assert!(s.form.is_some(), "testing must keep the form open");
    }

    #[test]
    fn set_status_shows_on_footer() {
        let mut s = ConnectionsState::default();
        let (s, _i, _e, dirty) = update(
            ConnectionsMessage::SetStatus {
                status: "Test OK".into(),
                kind: ConnectionStatusKind::Success,
            },
            std::mem::take(&mut s),
        );
        assert!(dirty);
        assert_eq!(s.status.as_deref(), Some("Test OK"));
        assert_eq!(s.status_kind, ConnectionStatusKind::Success);
    }

    #[test]
    fn commit_form_ignored_in_insert_mode() {
        let mut s = ConnectionsState {
            instance_name: "inst".into(),
            form: Some(ConnectionForm {
                name: "main".into(),
                mode: FormMode::Insert,
                ..Default::default()
            }),
            ..Default::default()
        };
        // A stray CommitForm in insert mode must not save (no effects) and must
        // keep the form open; Enter is meant to commit the field instead.
        let (s, _i, effects, dirty) =
            update(ConnectionsMessage::CommitForm, std::mem::take(&mut s));
        assert!(!dirty);
        assert!(
            effects.is_empty(),
            "insert-mode Enter must not save the whole form"
        );
        assert!(s.form.is_some());
    }

    #[test]
    fn test_form_on_edit_with_blank_password_borrows_stored_password() {
        // Editing "old"; username modified to "newuser", password left blank.
        // The form test must push `TestEditedFormConnection` with the CURRENT
        // username/database + the ORIGINAL name (so the store borrows the saved
        // password) — it must NOT bounce off the unchanged saved connection,
        // which would test the OLD value instead of the modified one.
        let mut s = ConnectionsState {
            instance_name: "inst".into(),
            form: Some(ConnectionForm {
                name: "old".into(),
                username: "newuser".into(),
                database: "appdb".into(),
                password: String::new(),
                edit_original_name: Some("old".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (_s, _i, effects, dirty) = update(ConnectionsMessage::TestForm, std::mem::take(&mut s));
        assert!(
            !dirty,
            "starting a form test must not trigger a redundant repaint"
        );
        assert!(
            effects.iter().any(|e| matches!(
                e,
                ConnectionsEffect::TestEditedFormConnection { instance_name, original_name, connection }
                    if instance_name == "inst"
                       && original_name == "old"
                       && connection.name == "old"
                       && connection.username == "newuser"
                       && connection.database == "appdb"
            )),
            "expected TestEditedFormConnection with modified fields, got {effects:?}"
        );
    }

    #[test]
    fn test_selected_emits_test_effect() {
        let mut s = ConnectionsState {
            instance_name: "inst".into(),
            connections: vec![dbm_store::InstanceConnection {
                id: "c1".into(),
                instance_id: "inst".into(),
                name: "main".into(),
                username: "postgres".into(),
                database: "postgres".into(),
                has_password: false,
                ssl_mode: "prefer".into(),
                env_label: None,
                created_at: "now".into(),
                updated_at: "now".into(),
                test_succeeded_at: None,
                test_failed_at: None,
            }],
            ..Default::default()
        };
        let (_s, _i, effects, dirty) =
            update(ConnectionsMessage::TestSelected, std::mem::take(&mut s));
        // Starting the async test renders nothing new, so it must not mark the
        // round dirty (a redundant repaint while `t` is held).
        assert!(
            !dirty,
            "starting a list test must not trigger a redundant repaint"
        );
        assert!(effects.iter().any(|e| matches!(
            e,
            ConnectionsEffect::TestConnection { instance_name, connection_name }
                if instance_name == "inst" && connection_name == "main"
        )));
    }

    #[test]
    fn test_complete_sets_status_and_reloads() {
        let mut s = ConnectionsState {
            instance_name: "inst".into(),
            ..Default::default()
        };
        let (s, _i, effects, dirty) = update(
            ConnectionsMessage::TestComplete {
                ok: true,
                error: None,
            },
            std::mem::take(&mut s),
        );
        // The status and reloaded rows are rendered together by the `Loaded`
        // repaint, so `TestComplete` itself must not mark the round dirty (that
        // would repaint the whole unchanged list -> ~66% waste on each test).
        assert!(
            !dirty,
            "TestComplete must not trigger a redundant list repaint"
        );
        let status = s.status.as_deref().expect("status set");
        assert!(status.ends_with("Test OK"), "{status}");
        assert_eq!(s.status_kind, ConnectionStatusKind::Success);
        assert!(effects.iter().any(|e| matches!(
            e,
            ConnectionsEffect::LoadConnections { instance_name } if instance_name == "inst"
        )));
    }

    #[test]
    fn test_form_and_selected_set_cooldown() {
        let mut s = ConnectionsState {
            instance_name: "inst".into(),
            form: Some(ConnectionForm {
                name: "main".into(),
                ..Default::default()
            }),
            connections: vec![dbm_store::InstanceConnection {
                id: "c1".into(),
                instance_id: "inst".into(),
                name: "main".into(),
                username: "postgres".into(),
                database: "postgres".into(),
                has_password: false,
                ssl_mode: "prefer".into(),
                env_label: None,
                created_at: "now".into(),
                updated_at: "now".into(),
                test_succeeded_at: None,
                test_failed_at: None,
            }],
            ..Default::default()
        };
        // The form test arms the 1s cooldown.
        let (mut s, _i, _e, _) = update(ConnectionsMessage::TestForm, std::mem::take(&mut s));
        assert!(s.test_cooldown_until.is_some());
        // The list test also arms it.
        let (s, _i, _e, _) = update(ConnectionsMessage::TestSelected, std::mem::take(&mut s));
        assert!(s.test_cooldown_until.is_some());
    }

    #[test]
    fn open_and_close_form_clear_test_status() {
        // Opening the form clears any prior test status.
        let mut s = ConnectionsState {
            status: Some("[t] Test OK".into()),
            status_kind: ConnectionStatusKind::Success,
            ..Default::default()
        };
        let (mut s, _i, _e, _) = update(ConnectionsMessage::BeginAdd, std::mem::take(&mut s));
        assert!(s.form.is_some());
        assert!(s.status.is_none(), "opening the form clears test status");
        assert_eq!(s.status_kind, ConnectionStatusKind::Idle);

        // Cancelling the form also clears it.
        let (mut s, _i, _e, _) = update(
            ConnectionsMessage::SetStatus {
                status: "[t] Test OK".into(),
                kind: ConnectionStatusKind::Success,
            },
            std::mem::take(&mut s),
        );
        assert!(s.status.is_some());
        let (s, _i, _e, _) = update(ConnectionsMessage::CancelForm, std::mem::take(&mut s));
        assert!(s.form.is_none());
        assert!(s.status.is_none(), "closing the form clears test status");
    }

    #[test]
    fn commit_form_passes_the_selected_ssl_mode() {
        let mut s = ConnectionsState::default();
        s.instance_name = "inst".into();
        s.form = Some(ConnectionForm {
            name: "conn".into(),
            ssl_mode: "require".into(),
            ..Default::default()
        });
        let (_s, _i, effects, _dirty) =
            update(ConnectionsMessage::CommitForm, std::mem::take(&mut s));
        match effects.first() {
            Some(ConnectionsEffect::AddConnection { connection, .. }) => {
                assert_eq!(connection.ssl_mode.as_deref(), Some("require"));
            }
            _ => panic!("expected an AddConnection effect"),
        }
    }

    #[test]
    fn cycle_ssl_mode_only_applies_on_the_selector_field() {
        let mut s = ConnectionsState::default();
        s.form = Some(ConnectionForm {
            field: FormField::SslMode,
            ssl_mode: "disable".into(),
            ..Default::default()
        });
        let (s, _i, _e, dirty) =
            update(ConnectionsMessage::CycleSslMode(1), std::mem::take(&mut s));
        assert!(dirty);
        assert_eq!(s.form.unwrap().ssl_mode, "prefer");

        let mut s = ConnectionsState::default();
        s.form = Some(ConnectionForm {
            field: FormField::Name,
            ..Default::default()
        });
        let (s, _i, _e, dirty) =
            update(ConnectionsMessage::CycleSslMode(1), std::mem::take(&mut s));
        assert!(!dirty, "cycling is ignored on text fields");
        assert_eq!(s.form.unwrap().ssl_mode, "disable");
    }

    #[test]
    fn test_form_passes_the_selected_ssl_mode() {
        let mut s = ConnectionsState::default();
        s.instance_name = "inst".into();
        s.form = Some(ConnectionForm {
            name: "conn".into(),
            ssl_mode: "verify-full".into(),
            ..Default::default()
        });
        let (_s, _i, effects, _dirty) =
            update(ConnectionsMessage::TestForm, std::mem::take(&mut s));
        match effects.first() {
            Some(ConnectionsEffect::TestFormConnection { connection, .. }) => {
                assert_eq!(connection.ssl_mode.as_deref(), Some("verify-full"));
            }
            _ => panic!("expected a TestFormConnection effect"),
        }
    }
}
