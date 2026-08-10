//! Instance connections feature update.

use dbm_store::NewInstanceConnection;

use super::msg::ConnectionsMessage;
use super::state::{ConnectionStatusKind, ConnectionsState, FormMode};
use super::intent::ConnectionsIntent;
use super::effect::ConnectionsEffect;

/// Update the connections panel state. Pure by-value transition.
///
/// The returned `bool` is `dirty`: whether the rendered panel changed.
/// Load-only and delete messages change nothing locally (the later `Loaded`
/// repaint); form/navigation messages report per actual change.
pub fn update(
    msg: ConnectionsMessage,
    mut state: ConnectionsState,
) -> (ConnectionsState, Vec<ConnectionsIntent>, Vec<ConnectionsEffect>, bool) {
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
            // Repaint only when the list (or the cursor reset) actually changed,
            // so a refresh that reloads identical data does not redraw.
            let dirty = state.connections != connections || state.cursor != 0;
            state.connections = connections;
            state.cursor = 0;
            dirty
        }
        ConnectionsMessage::MoveUp => state.move_up(),
        ConnectionsMessage::MoveDown => state.move_down(),
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
            let changed = if let Some(idx) = Some(state.cursor) {
                state.begin_edit(idx)
            } else {
                false
            };
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
                ssl_mode: None,
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
            intents.push(ConnectionsIntent::ConnectionsChanged);
            true
        }
        ConnectionsMessage::Saved => {
            // The add/edit succeeded: close the form and reload the list so the
            // new/updated row appears (and the connection is visible in both the
            // list and the store).
            state.form = None;
            let instance_name = state.instance_name.clone();
            effects.push(ConnectionsEffect::LoadConnections { instance_name });
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
            let connection = NewInstanceConnection {
                name: form.name.trim().to_string(),
                username: form.username.clone(),
                database: form.database.clone(),
                password: if form.password.is_empty() {
                    None
                } else {
                    Some(form.password.clone())
                },
                ssl_mode: None,
                env_label: None,
            };
            let instance_name = state.instance_name.clone();
            effects.push(ConnectionsEffect::TestFormConnection {
                instance_name,
                connection,
            });
            // Starting the async test does not change any rendered state: the
            // result only lands when `TestResult`/`TestComplete` arrives. Marking
            // this dirty would trigger a redundant repaint (a held `t` wastes a
            // redraw every second with nothing visibly changed), so keep it false
            // and let the completion message repaint with the actual result.
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
                    instance_name,
                    connection_name,
                });
                intents.push(ConnectionsIntent::ConnectionsChanged);
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
        ConnectionsMessage::BeginFieldInsert => state.begin_field_insert(),
        ConnectionsMessage::CommitFieldInsert => state.commit_field_insert(),
        ConnectionsMessage::CancelFieldInsert => state.cancel_field_insert(),
        ConnectionsMessage::ClearFieldAndInsert => state.clear_field_and_insert(),
        ConnectionsMessage::SetPendingD => state.set_pending_d(),
        ConnectionsMessage::FormChar(c) if !c.is_control() => state.form_insert_char(c),
        ConnectionsMessage::FormChar(_) => false,
        ConnectionsMessage::FormBackspace => state.form_backspace(),
    };
    (state, intents, effects, dirty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::instance_workspace::connections::state::ConnectionForm;

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
    fn saved_closes_form_and_reloads() {
        let mut s = ConnectionsState {
            instance_name: "inst".into(),
            form: Some(ConnectionForm {
                name: "main".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (s, _i, effects, dirty) = update(ConnectionsMessage::Saved, std::mem::take(&mut s));
        // The form closes and the list reloads so the new row appears.
        assert!(dirty);
        assert!(s.form.is_none());
        assert!(effects
            .iter()
            .any(|e| matches!(e, ConnectionsEffect::LoadConnections { instance_name } if instance_name == "inst")));
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
        let (mut s, _i, _e, dirty) = update(ConnectionsMessage::BeginFieldInsert, std::mem::take(&mut s));
        assert!(dirty);
        assert_eq!(s.form.as_ref().unwrap().mode, FormMode::Insert);
        // Type into the field while in insert mode.
        let (mut s, _i, _e, _) = update(ConnectionsMessage::FormChar('2'), std::mem::take(&mut s));
        assert_eq!(s.form.as_ref().unwrap().name, "main2");
        // Cancel the field edit reverts to the snapshot.
        let (s, _i, _e, dirty) = update(ConnectionsMessage::CancelFieldInsert, std::mem::take(&mut s));
        assert!(dirty);
        assert_eq!(s.form.as_ref().unwrap().mode, FormMode::Normal);
        assert_eq!(s.form.as_ref().unwrap().name, "main");
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
        assert!(!dirty, "starting a form test must not trigger a redundant repaint");
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
        let (s, _i, effects, dirty) = update(ConnectionsMessage::CommitForm, std::mem::take(&mut s));
        assert!(!dirty);
        assert!(effects.is_empty(), "insert-mode Enter must not save the whole form");
        assert!(s.form.is_some());
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
        let (_s, _i, effects, dirty) = update(
            ConnectionsMessage::TestSelected,
            std::mem::take(&mut s),
        );
        // Starting the async test renders nothing new, so it must not mark the
        // round dirty (a redundant repaint while `t` is held).
        assert!(!dirty, "starting a list test must not trigger a redundant repaint");
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
        assert!(!dirty, "TestComplete must not trigger a redundant list repaint");
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
}
