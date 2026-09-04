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
    /// Move the cursor to a specific visible row (single click). Unlocks the
    /// discover-style anchor so the viewport follows the new cursor.
    JumpTo { row: usize },
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
    /// A save succeeded: close the form and reload the list.
    Saved,
    /// A save failed with `error`: keep the form open and show it on the footer.
    SaveError(String),
    /// Set the pane footer status text and its color kind (e.g. a test result).
    SetStatus {
        status: String,
        kind: super::state::ConnectionStatusKind,
    },
    /// A list test completed: show the result and reload so the row's test
    /// timestamps and color update.
    TestComplete { ok: bool, error: Option<String> },
    /// Test the form's current values against the database (the form's `t`).
    TestForm,
    /// Test the selected saved connection (the list's `t`).
    TestSelected,
    /// Move the form field cursor.
    FormField(FormField),
    /// A mouse click on a form field line: a single click selects the field,
    /// a double click enters insert mode on it (matching the original dbm's
    /// form field click handling). `is_double` is computed by the app loop's
    /// shared double-click detector.
    FormClick { field: FormField, is_double: bool },
    /// Enter insert mode on the current form field (`i`), snapshotting its value
    /// so `Esc` can revert it.
    BeginFieldInsert,
    /// Commit the current field and return to normal mode (`Enter`).
    CommitFieldInsert,
    /// Cancel the current field edit, reverting it to its pre-edit value
    /// (`Esc` in insert mode).
    CancelFieldInsert,
    /// Clear the current field and enter insert mode (`d` `d`), snapshotting the
    /// old value for `Esc` to restore.
    ClearFieldAndInsert,
    /// Record the first `d` press (the `dd` clear-field chord needs two `d`
    /// within a short window, matching the original dbm).
    SetPendingD,
    /// Insert a character into the active form field.
    FormChar(char),
    /// Backspace in the active form field.
    FormBackspace,
    /// Set the vertical scroll position (0 = top row visible). Issued by the
    /// v_scrollbar drag handler; sets `scroll_locked = true`.
    SetVScroll { position: usize },
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
