//! Instance connections feature messages.

use super::state::FormField;

/// The actual connections panel messages.
#[derive(Debug, Clone)]
pub enum ConnectionsMessage {
    /// Load the connections for `instance_name`.
    Load { instance_name: String },
    /// Re-load the connections for the current instance (after a mutation).
    Reload,
    /// The store returned the connections.
    Loaded { connections: Vec<dbm_store::InstanceConnection> },
    /// Move the connection cursor up.
    MoveUp,
    /// Move the connection cursor down.
    MoveDown,
    /// Open the add-connection form.
    BeginAdd,
    /// Open the edit form for the connection at the cursor.
    BeginEdit,
    /// Discard the open form.
    CancelForm,
    /// Commit the open form (add or edit).
    CommitForm,
    /// Delete a specific connection (dispatched from the delete-confirm modal).
    DeleteConnection { instance_name: String, connection_name: String },
    /// Move the form field cursor.
    FormField(FormField),
    /// Insert a character into the active form field.
    FormChar(char),
    /// Backspace in the active form field.
    FormBackspace,
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone)]
pub enum ConnectionsMsg {
    Message(ConnectionsMessage),
}

impl From<ConnectionsMessage> for ConnectionsMsg {
    fn from(m: ConnectionsMessage) -> Self {
        ConnectionsMsg::Message(m)
    }
}
