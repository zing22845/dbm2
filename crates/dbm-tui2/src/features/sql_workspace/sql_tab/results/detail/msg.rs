//! Results detail sub-module messages.

use crossterm::event::KeyEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailMessage {
    /// Scroll the detail body by `delta` display rows.
    Scroll { delta: i32 },
    /// Set the detail body's scroll position absolutely (0 = top) — issued by
    /// the detail scrollbar's click-to-jump and drag, mirroring the list's
    /// `SetVScroll`. Moves the focused editor's viewport or the read-only
    /// preview's scroll offset depending on which body is showing.
    SetVScroll { position: usize },
    /// Set the detail draft text (edited cell value).
    SetDraft { text: String },
    /// Load a cell value as the draft baseline.
    LoadCell { value: String },
    /// Clear the draft state (on exit edit or rollback).
    ClearDraft,
    /// Forward a key into the focused cell editor (typing / vim motion / Esc
    /// mode downgrade). Only consumed while the detail editor is focused.
    KeyEvent {
        key: KeyEvent,
        tracked_caps_lock: bool,
    },
    /// A mouse Down/Drag/Up on the focused cell editor's text, decoded by the
    /// pointer layer through edtui against a scratch copy (with the rendered
    /// hit area fed in). `update` just applies cursor/mode/selection — the draft
    /// text never changes, so a selection can stay highlighted for the copy key.
    MouseGesture {
        outcome: super::super::super::editor::mouse::MouseGestureOutcome,
    },
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
