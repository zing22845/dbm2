//! Discovery targets editor feature state.

use dbm_discovery::{parse_port_spec, validate_host};

/// A single editable scan target row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRow {
    pub host: String,
    pub ports_spec: String,
}

/// The editable column of a target row currently in focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetCol {
    #[default]
    Host,
    Ports,
}

/// State for the targets editor.
///
/// Owns the editable target list plus the editing machinery (inline cell edit,
/// undo/redo stacks) and the cursor position. Pure-functional update is in
/// `super::update`. The list's scroll offset is stored here and adjusted
/// after cursor movement to keep the cursor visible (matching the original
/// dbm's `ensure_targets_visible`).
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
    /// Raw payload of the last paste. Re-feeding the identical payload (a held
    /// Cmd+V auto-repeat) is de-duped so the same targets are not appended
    /// twice — matching the original dbm.
    pub last_paste_content: Option<String>,
    /// One-line feedback for the last paste/undo/redo action (e.g. how many
    /// rows were added / undone). Rendered as a status line in the targets
    /// footer and cleared on the next action or a fresh edit.
    pub status: Option<String>,
    /// First visible row index in the scroll viewport. Only changes when the
    /// cursor would move outside the viewport — the scroll does NOT follow
    /// every cursor movement (matching the original dbm).
    pub scroll_offset: usize,
    /// Last computed viewport height (data rows, excluding the header).
    /// Updated from the render loop so update handlers can clamp scroll.
    pub target_viewport: usize,
    /// When true, cursor-anchored viewport scrolling is skipped so a manual
    /// scroll (scrollbar drag / SetVScroll) is honoured until the next cursor
    /// move. Same pattern as history/results ListStates.
    pub scroll_locked: bool,
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
        let row = self
            .targets
            .get(row)
            .ok_or_else(|| "target row out of range".to_string())?;
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

    /// Whether any target row has a loopback host (drives the footer note).
    pub fn has_loopback(&self) -> bool {
        self.targets
            .iter()
            .any(|r| dbm_discovery::is_loopback_host(&r.host))
    }

    /// Adjust `scroll_offset` so that `row` remains visible within the
    /// viewport. The scroll is NOT forced to follow every cursor movement —
    /// it only changes when `row` would fall outside `[scroll_offset,
    /// scroll_offset + viewport)`. Mirrors the original dbm's
    /// `ensure_targets_visible`.
    pub fn ensure_row_visible(&mut self, row: usize) {
        let viewport = self.target_viewport;
        if viewport == 0 {
            return;
        }
        let viewport = viewport.max(1);
        let total = self.targets.len();
        if total == 0 || viewport >= total {
            self.scroll_offset = 0;
            return;
        }
        let scroll = self.scroll_offset.min(total.saturating_sub(1));
        if row < scroll {
            self.scroll_offset = row;
        } else if row >= scroll + viewport {
            self.scroll_offset = row + 1 - viewport;
        }
        // else: row is already visible, do not change scroll
    }

    /// Clamp `scroll_offset` to a valid range after list mutations.
    pub fn clamp_scroll(&mut self) {
        let viewport = self.target_viewport;
        if viewport == 0 {
            return;
        }
        let viewport = viewport.max(1);
        let total = self.targets.len();
        if total == 0 {
            self.scroll_offset = 0;
            return;
        }
        let max_start = total.saturating_sub(viewport);
        self.scroll_offset = self.scroll_offset.min(max_start);
        self.row = self.row.min(total.saturating_sub(1));
    }
}
