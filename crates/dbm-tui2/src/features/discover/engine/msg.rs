//! Engine selector feature messages.

/// The actual engine selector messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineMessage {
    /// Select the given engine.
    Select(dbm_core::Engine),
}

/// Feature message envelope (central-router compatible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineMsg {
    Message(EngineMessage),
}

impl From<EngineMessage> for EngineMsg {
    fn from(m: EngineMessage) -> Self {
        EngineMsg::Message(m)
    }
}
