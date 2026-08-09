//! Engine selector feature messages.

/// The actual engine selector messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineMessage {
    /// Select the given engine.
    Select(dbm_core::Engine),
    /// `e`/`Enter` on the engine pane is a no-op today (only one engine), so
    /// surface the "only Postgres is available" note on the engine footer.
    ShowOnlyEngineNote,
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
