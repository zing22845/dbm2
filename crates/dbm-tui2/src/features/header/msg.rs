//! Header feature messages.

/// The actual header messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderMessage {
    /// Move the button cursor one step left (wrapping).
    MoveLeft,
    /// Move the button cursor one step right (wrapping).
    MoveRight,
    /// Activate the currently focused button.
    Activate,
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderMsg {
    /// A real header message.
    Message(HeaderMessage),
}

impl From<HeaderMessage> for HeaderMsg {
    fn from(m: HeaderMessage) -> Self {
        HeaderMsg::Message(m)
    }
}
