//! Results detail sub-module state.
//!
//! The detail section previews the selected cell's value. It owns the scroll
//! offset and the inline-edit draft state. The detail pane width lives in the
//! `splitter` sub-feature (`super::splitter::state`).
//!
//! While the whole-result edit session is active the detail can take keyboard
//! focus (`focused`) and host an edtui editor over a *draft* of the current
//! cell (see [`DetailEditor`]): typing only mutates the draft, Ctrl+S saves it
//! back into the edit session's `dirty_cells`, Ctrl+U discards it, Esc leaves
//! the editor (mirroring the original dbm's detail editor). `baseline` is the
//! cell value the draft started from; `dirty` compares them.

use super::super::detail_edit::detail_draft_dirty;

/// The live edtui editor + its key handler for an in-focus cell draft.
///
/// Owned here (rather than on the tab alongside the SQL editor) so each
/// results feature carries its own draft cursor/mode/undo. `edtui` types do
/// not implement `Debug`, so [`DetailState`] implements it manually.
#[derive(Clone)]
pub struct DetailEditor {
    /// The draft buffer / cursor (edtui).
    pub editor: edtui::EditorState,
    /// The shared key handler (vim + readline insert chords).
    pub handler: edtui::EditorEventHandler,
}

impl DetailEditor {
    /// Build an editor seeded with `value`. Starts in **Normal** mode (like the
    /// SQL editor and the original dbm's `apply_normal_on_enter`): the user
    /// presses `i`/`a`/`o` to enter Insert and edit the draft, and Esc returns
    /// to Normal before leaving the editor.
    pub fn new(value: &str) -> Self {
        let mut editor = crate::common::editor::new_editor(value);
        editor.selection = None;
        editor.set_scroll_locked(false);
        DetailEditor {
            editor,
            handler: crate::common::editor::new_editor_handler(),
        }
    }
}

/// State for the detail preview / cell-draft editor.
#[derive(Clone, Default)]
pub struct DetailState {
    /// Vertical scroll offset of the detail body (read-only preview).
    pub scroll: usize,
    /// The detail draft's baseline (cell value at load) for dirty detection.
    pub baseline: String,
    /// The detail draft's current text (edited value).
    pub draft: String,
    /// Whether the detail draft is dirty (differs from baseline).
    pub dirty: bool,
    /// Whether an unsaved detail draft blocks leaving Detail.
    pub leave_warning: bool,
    /// Whether the detail editor currently owns the keyboard. Only meaningful
    /// while the whole-result edit session is active; when `false` the detail
    /// is a read-only preview that follows the table selection.
    pub focused: bool,
    /// The active cell-draft editor (present only while `focused`).
    pub editor: Option<DetailEditor>,
}

impl std::fmt::Debug for DetailState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DetailState")
            .field("scroll", &self.scroll)
            .field("baseline", &self.baseline)
            .field("draft", &self.draft)
            .field("dirty", &self.dirty)
            .field("leave_warning", &self.leave_warning)
            .field("focused", &self.focused)
            .field(
                "editor",
                &self
                    .editor
                    .as_ref()
                    .map(|e| crate::common::editor::editor_text(&e.editor)),
            )
            .finish()
    }
}

impl DetailState {
    /// Clamp the scroll to the number of wrapped display rows.
    pub fn clamp_scroll(&mut self, row_count: usize, viewport: usize) {
        let max = row_count.saturating_sub(viewport.max(1));
        self.scroll = self.scroll.min(max);
    }

    /// Load a cell value as the draft baseline (used when entering edit / when
    /// the selection moves while the detail is a read-only preview).
    pub fn load_cell(&mut self, value: &str) {
        self.baseline = value.to_string();
        self.draft = value.to_string();
        self.dirty = false;
        self.leave_warning = false;
    }

    /// The current draft text: the editor buffer while focused, else `draft`.
    pub fn current_text(&self) -> String {
        self.editor
            .as_ref()
            .map(|e| crate::common::editor::editor_text(&e.editor))
            .unwrap_or_else(|| self.draft.clone())
    }

    /// Whether an unsaved draft exists that would be lost by leaving Detail.
    pub fn has_unsaved_draft(&self) -> bool {
        self.focused && self.dirty
    }

    /// Focus the detail editor on the current cell: build a fresh draft editor
    /// from `value` and record it as the baseline.
    pub fn focus_editor(&mut self, value: &str) {
        self.baseline = value.to_string();
        self.draft = value.to_string();
        self.dirty = false;
        self.leave_warning = false;
        self.scroll = 0;
        self.editor = Some(DetailEditor::new(value));
        self.focused = true;
    }

    /// Leave editor focus back to the table. Only valid when the draft is not
    /// dirty (the leave guard lives in the results update).
    pub fn unfocus(&mut self) {
        self.focused = false;
        self.editor = None;
    }

    /// Clear the draft state (used when exiting edit, rolling back, or closing
    /// the detail). Drops any focused editor.
    pub fn clear_draft(&mut self) {
        self.baseline.clear();
        self.draft.clear();
        self.dirty = false;
        self.leave_warning = false;
        self.focused = false;
        self.editor = None;
    }

    /// Reset scroll and leave warning (used when closing detail). Keeps the
    /// draft/editor so re-opening an edit session can inspect them.
    pub fn reset_for_close(&mut self) {
        self.scroll = 0;
        self.leave_warning = false;
    }

    /// Recompute `draft`/`dirty` from the current editor buffer and refresh the
    /// baseline diff highlights. Called after every editor key.
    pub fn sync_draft_from_editor(&mut self) {
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        self.draft = crate::common::editor::editor_text(&editor.editor);
        self.dirty = detail_draft_dirty(&self.draft, &self.baseline);
        crate::common::editor::refresh_detail_dirty_highlights(&mut editor.editor, &self.baseline);
    }
}
