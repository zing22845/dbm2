//! Explorer objects feature messages.

/// The actual objects (object tree) messages.
#[derive(Debug, Clone)]
pub enum ObjectsMessage {
    /// Move the cursor up.
    MoveUp,
    /// Move the cursor down.
    MoveDown,
    /// Toggle expansion of the current row.
    ToggleExpand,
    /// Activate the current row (open an object, e.g. a table).
    Select,
    /// Rebind the tree to an instance/connection.
    Bind { instance: String, connection: String },
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone)]
pub enum ObjectsMsg {
    Message(ObjectsMessage),
}

impl From<ObjectsMessage> for ObjectsMsg {
    fn from(m: ObjectsMessage) -> Self {
        ObjectsMsg::Message(m)
    }
}
