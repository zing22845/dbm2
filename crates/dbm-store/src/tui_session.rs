//! Typed snapshot types for persisting and restoring the TUI session.
//!
//! The TUI session (open tabs, tree expansion, split ratios, focus, etc.) is
//! serialized to JSON and stored in the key-value store under `tui_session`.
//! These types define the on-disk schema.

use crate::{StoreResult, kv_store};
use serde::{Deserialize, Serialize};

/// Schema version for the serialized TUI session snapshot.
///
/// Bump this when the snapshot layout changes in a backward-incompatible way so
/// that older snapshots are rejected instead of being partially applied.
pub const TUI_SESSION_VERSION: u32 = 1;

/// A selection in the explorer tree, identified by instance/connection name
/// rather than transient indices (instances/connections can be reordered).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TuiTreeSelection {
    Instance { instance: String },
    Connection { instance: String, connection: String },
}

/// Snapshot of the explorer tree state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuiTreeSnapshot {
    /// Names of instances that were expanded when the session was saved.
    pub expanded_instances: Vec<String>,
    /// The currently highlighted node.
    pub cursor: Option<TuiTreeSelection>,
    /// The node backing the active workspace (if any).
    pub active_workspace: Option<TuiTreeSelection>,
    /// Expanded keys of the objects tree (database / database+schema /
    /// database+schema+kind), preserved across restarts. Defaults to empty for
    /// snapshots written before this field existed.
    #[serde(default)]
    pub expanded_objects: Vec<String>,
    /// The connection the objects tree was bound to when saved (empty = tree
    /// unbound). Used to re-apply `expanded_objects` only to the matching
    /// connection after a restart.
    #[serde(default)]
    pub objects_bound_instance: String,
    #[serde(default)]
    pub objects_bound_connection: String,
}

/// Snapshot of the instance-workspace sub-pane state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuiInstanceWorkspaceSnapshot {
    /// Active instance-workspace section (e.g. "overview", "connections").
    pub section: String,
    /// Cursor position within the connections list.
    pub connections_cursor: usize,
}

/// Snapshot of a single open SQL tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuiTabSnapshot {
    pub instance: String,
    pub connection: String,
    pub sequence: u32,
    pub sql: String,
    /// Horizontal split ratio between editor and results, as a percentage (0-100).
    pub split_ratio: u8,
    /// Fixed-width history pane width in columns.
    pub history_pane_width: u16,
    /// Fixed-width detail pane width in columns.
    pub detail_pane_width: u16,
    pub database: String,
    pub schema: String,
    pub complete_table_names: bool,
}

/// Full snapshot of the TUI session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuiSessionSnapshot {
    pub version: u32,
    pub focus: String,
    pub tree_width: u16,
    pub tree: TuiTreeSnapshot,
    pub tabs: Vec<TuiTabSnapshot>,
    pub active_tab: Option<usize>,
    pub instance_workspace: Option<TuiInstanceWorkspaceSnapshot>,
    /// Discover-targets split ratio as a percentage (0-100).
    pub discover_targets_ratio: u8,
    /// Explorer split ratio as a percentage (0-100).
    pub explorer_split_ratio: u8,
    pub explorer_pane: String,
}

/// Load the persisted TUI session snapshot.
///
/// Returns `Ok(None)` when no session has been saved yet. A snapshot with a
/// mismatched [`TUI_SESSION_VERSION`] is treated as absent so the caller falls
/// back to a fresh session.
pub fn load_tui_session() -> StoreResult<Option<TuiSessionSnapshot>> {
    let Some(json) = kv_store::kv_load_tui_session()? else {
        return Ok(None);
    };
    let snapshot: TuiSessionSnapshot = match serde_json::from_str(&json) {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    if snapshot.version != TUI_SESSION_VERSION {
        return Ok(None);
    }
    Ok(Some(snapshot))
}

/// Persist the given TUI session snapshot as JSON.
pub fn save_tui_session(snapshot: &TuiSessionSnapshot) -> StoreResult<()> {
    let json = serde_json::to_string(snapshot)?;
    kv_store::kv_save_tui_session(&json)
}
