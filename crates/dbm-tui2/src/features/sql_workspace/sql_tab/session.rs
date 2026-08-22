//! Per-tab session state.
//!
//! Each SQL tab owns an independent `TabSession`: which database connection it
//! is bound to, which database/schema it currently operates on, and (in the
//! future) transaction state, prepared statements and editor position. A
//! session is the unit of persistence — a tab's session can be serialized and
//! restored across app restarts, so a tab keeps its context after a relaunch.

/// The identity and connection context of a single SQL tab.
///
/// `id` is a stable identifier that survives tab reordering/removal, distinct
/// from the tab's index in `SqlTabState::tabs`. The remaining fields are the
/// persistence-relevant context that will be (de)serialized once real session
/// data exists; they are placeholders today.
#[derive(Debug, Clone)]
#[derive(Default)]
pub struct TabSession {
    /// Stable identity across app restarts (used for persistence and routing).
    pub id: usize,
    /// Per-connection tab number, starting at 1 (mirrors original dbm's
    /// `sequence` — each connection counts its own tabs independently).
    pub sequence: usize,
    /// Identifier of the database connection this tab is bound to, if any
    /// (the store's string id).
    pub connection_id: Option<String>,
    /// The instance the connection belongs to (display name), if any.
    pub instance: Option<String>,
    /// The connection name (display name), if any.
    pub connection: Option<String>,
    /// Currently selected database, if any.
    pub database: Option<String>,
    /// Currently selected schema, if any.
    pub schema: Option<String>,
}

/// The `(instance, connection)` key used to look up history entries, mirroring
/// how history is recorded per connection.
pub fn session_view_key(session: &TabSession) -> (String, String) {
    let instance = session.instance.clone().unwrap_or_default();
    let connection = session
        .connection
        .clone()
        .or_else(|| session.connection_id.clone())
        .unwrap_or_default();
    (instance, connection)
}

