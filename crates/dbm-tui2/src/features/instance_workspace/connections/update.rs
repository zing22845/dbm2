//! Instance connections feature update.

use dbm_store::NewInstanceConnection;

use super::msg::ConnectionsMessage;
use super::state::{ConnectionsState, FormField};
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
        ConnectionsMessage::BeginAdd => state.begin_add(),
        ConnectionsMessage::BeginEdit => {
            if let Some(idx) = Some(state.cursor) {
                state.begin_edit(idx)
            } else {
                false
            }
        }
        ConnectionsMessage::CancelForm => {
            let changed = state.form.is_some();
            state.form = None;
            changed
        }
        ConnectionsMessage::CommitForm => {
            let Some(form) = state.form.take() else {
                return (state, intents, effects, false);
            };
            if form.name.trim().is_empty() {
                // Name is required; keep the form open for correction.
                state.form = Some(form);
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
            match form.edit_original_name {
                Some(original_name) => effects.push(ConnectionsEffect::EditConnection {
                    instance_name,
                    original_name,
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
            if let Some(form) = state.form.as_mut() {
                let changed = form.field != field;
                form.field = field;
                changed
            } else {
                false
            }
        }
        ConnectionsMessage::FormChar(c) if !c.is_control() => {
            if let Some(form) = state.form.as_mut() {
                let field_value = form_field_mut(form, form.field);
                field_value.push(c);
                true
            } else {
                false
            }
        }
        ConnectionsMessage::FormChar(_) => false,
        ConnectionsMessage::FormBackspace => {
            if let Some(form) = state.form.as_mut() {
                let field_value = form_field_mut(form, form.field);
                let changed = !field_value.is_empty();
                field_value.pop();
                changed
            } else {
                false
            }
        }
    };
    (state, intents, effects, dirty)
}

fn form_field_mut(form: &mut super::state::ConnectionForm, field: FormField) -> &mut String {
    match field {
        FormField::Name => &mut form.name,
        FormField::Username => &mut form.username,
        FormField::Database => &mut form.database,
        FormField::Password => &mut form.password,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
