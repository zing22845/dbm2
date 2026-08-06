//! Discovery targets editor feature state.

use dbm_discovery::{parse_port_spec, validate_host};

/// A single editable scan target row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRow {
    pub host: String,
    pub ports_spec: String,
}

/// The editable column of a target row currently in focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum TargetCol {
    #[default]
    Host,
    Ports,
}

/// State for the targets editor.
///
/// Owns the editable target list plus the editing machinery (inline cell edit,
/// undo/redo stacks) and the cursor/scroll position. Pure-functional update is
/// in `super::update`.
#[derive(Debug, Clone, Default)]
pub struct TargetsState {
    /// The editable target rows.
    pub targets: Vec<TargetRow>,
    /// Cursor row index.
    pub row: usize,
    /// Cursor column (Host/Ports).
    pub col: TargetCol,
    /// Whether an inline cell edit is in progress.
    pub editing: bool,
    /// Text buffer of the in-progress edit.
    pub edit_buf: String,
    /// Byte cursor position within `edit_buf`.
    pub edit_cursor: usize,
    /// Undo snapshots of the target list.
    pub undo_stack: Vec<Vec<TargetRow>>,
    /// Redo snapshots of the target list.
    pub redo_stack: Vec<Vec<TargetRow>>,
    /// Scroll offset of the targets list.
    pub scroll: usize,
}


impl TargetsState {
    /// A freshly opened discover modal starts with a loopback target.
    pub fn with_default_targets() -> Self {
        TargetsState {
            targets: vec![TargetRow {
                host: "127.0.0.1".into(),
                ports_spec: "5432,5433-5440".into(),
            }],
            ..TargetsState::default()
        }
    }

    /// Whether any target row has an empty host or port spec.
    pub fn has_empty_row(&self) -> bool {
        self.targets
            .iter()
            .any(|r| r.host.trim().is_empty() || r.ports_spec.trim().is_empty())
    }

    /// Resolve `(host, ports_spec)` to a parsed `DiscoveryTarget` if valid.
    pub fn resolve_target(&self, row: usize) -> Result<dbm_discovery::DiscoveryTarget, String> {
        let row = self.targets.get(row).ok_or_else(|| "target row out of range".to_string())?;
        let host = validate_host(&row.host)?;
        let ports = parse_port_spec(&row.ports_spec)?;
        Ok(dbm_discovery::DiscoveryTarget { host, ports })
    }

    /// Reset any in-progress edit.
    pub fn discard_edit(&mut self) {
        self.editing = false;
        self.edit_buf.clear();
        self.edit_cursor = 0;
    }
}
