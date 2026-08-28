//! History detail sub-feature messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailMessage {
    /// Scroll the detail body by `delta` lines (clamped to content).
    Scroll { delta: i32 },
    /// Scroll the detail body by half a page.
    ScrollPage { down: bool },
    /// Set the detail scroll to an absolute position (from scrollbar drag).
    SetScroll { position: usize },
    /// Pin a SQL entry open (entering recall mode).
    PinSql { sql: String },
    /// Reset scroll and pinned SQL.
    Reset,
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
