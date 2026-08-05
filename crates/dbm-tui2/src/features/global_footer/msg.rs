//! Global footer feature messages.

/// The actual global footer messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FooterMessage {
    /// Replace the footer status line with `status`.
    SetStatus(String),
    /// Clear the footer status line.
    ClearStatus,
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FooterMsg {
    Message(FooterMessage),
}

impl From<FooterMessage> for FooterMsg {
    fn from(m: FooterMessage) -> Self {
        FooterMsg::Message(m)
    }
}
