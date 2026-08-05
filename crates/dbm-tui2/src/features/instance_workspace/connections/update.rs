//! Instance connections feature update.

use dbm_store::NewInstanceConnection;

use super::msg::ConnectionsMessage;
use super::state::{ConnectionsState, FormField};
use super::intent::ConnectionsIntent;
use super::effect::ConnectionsEffect;

/// Update the connections panel state. Pure by-value transition.
pub fn update(
    msg: ConnectionsMessage,
    mut state: ConnectionsState,
) -> (ConnectionsState, Vec<ConnectionsIntent>, Vec<ConnectionsEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    match msg {
        ConnectionsMessage::Load { instance_name } => {
            effects.push(ConnectionsEffect::LoadConnections { instance_name });
        }
        ConnectionsMessage::Reload => {
            let instance_name = state.instance_name.clone();
            effects.push(ConnectionsEffect::LoadConnections { instance_name });
        }
        ConnectionsMessage::Loaded { connections } => {
            state.connections = connections;
            state.cursor = 0;
        }
        ConnectionsMessage::MoveUp => state.move_up(),
        ConnectionsMessage::MoveDown => state.move_down(),
        ConnectionsMessage::BeginAdd => state.begin_add(),
        ConnectionsMessage::BeginEdit => {
            if let Some(idx) = Some(state.cursor) {
                state.begin_edit(idx);
            }
        }
        ConnectionsMessage::CancelForm => state.form = None,
        ConnectionsMessage::CommitForm => {
            if let Some(form) = state.form.take() {
                if form.name.trim().is_empty() {
                    // Name is required; keep the form open for correction.
                    state.form = Some(form);
                    return (state, intents, effects);
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
            }
        }
        ConnectionsMessage::Delete => {
            if let Some(name) = state.selected_name() {
                let instance_name = state.instance_name.clone();
                if !instance_name.is_empty() {
                    effects.push(ConnectionsEffect::DeleteConnection {
                        instance_name,
                        connection_name: name,
                    });
                    intents.push(ConnectionsIntent::ConnectionsChanged);
                }
            }
        }
        ConnectionsMessage::FormField(field) => {
            if let Some(form) = state.form.as_mut() {
                form.field = field;
            }
        }
        ConnectionsMessage::FormChar(c) if !c.is_control() => {
            if let Some(form) = state.form.as_mut() {
                let field_value = form_field_mut(form, form.field);
                field_value.push(c);
            }
        }
        ConnectionsMessage::FormChar(_) => {}
        ConnectionsMessage::FormBackspace => {
            if let Some(form) = state.form.as_mut() {
                let field_value = form_field_mut(form, form.field);
                field_value.pop();
            }
        }
    }
    (state, intents, effects)
}

fn form_field_mut(form: &mut super::state::ConnectionForm, field: FormField) -> &mut String {
    match field {
        FormField::Name => &mut form.name,
        FormField::Username => &mut form.username,
        FormField::Database => &mut form.database,
        FormField::Password => &mut form.password,
    }
}
