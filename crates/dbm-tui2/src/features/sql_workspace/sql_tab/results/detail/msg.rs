//! Results detail sub-module messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailMessage {
    /// Scroll the detail body by `delta` display rows.
    Scroll { delta: i32 },
    /// Set the detail draft text (edited cell value).
    SetDraft { text: String },
    /// Load a cell value as the draft baseline.
    LoadCell { value: String },
    /// Clear the draft state (on exit edit or rollback).
    ClearDraft,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailMsg {
    Message(DetailMessage),
}

impl From<DetailMessage> for DetailMsg {
    fn from(m: DetailMessage) -> Self {
        DetailMsg::Message(m)
    }
}