//! Header feature messages.

/// The actual header messages. Empty in the skeleton; business logic will
/// add variants (e.g. toggle view, open command palette).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderMessage {}

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
