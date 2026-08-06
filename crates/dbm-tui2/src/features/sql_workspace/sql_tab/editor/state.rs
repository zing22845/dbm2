//! Editor feature state.
//!
//! Owns the SQL buffer + cursor via the vendored `edtui` editor and its key
//! handler, plus the context picker and SQL completion child sub-modules.

use std::collections::HashMap;

use super::context_picker::state::ContextPickerState;
use super::sql_completion::state::SqlCompletionState;
use super::sql_search::EditorSqlSearch;
use crate::common::editor;
use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;

/// Cached catalog metadata used to power SQL completion: the schema's table
/// names plus each table's columns. Loaded once when the tab binds to a
/// connection (`EditorEffect::LoadCompletionCatalog`), so typing a query can
/// offer table / column names without a DB round-trip per keystroke.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompletionCatalog {
    /// Every table in the tab's active schema (for `FROM` / table completion).
    pub tables: Vec<String>,
    /// Column metadata keyed by table name (for column completion).
    pub columns_by_table: HashMap<String, Vec<ColumnInfo>>,
}

/// The editor feature state. Not `Default`-derived: the `edtui::EditorState`
/// has no `Default` impl, so it is built via `editor::new_editor`. `Debug` is
/// implemented manually because `edtui::EditorState` / `EditorEventHandler` do
/// not implement it (we print the buffer text and cursor instead).
#[derive(Clone)]
pub struct EditorState {
    /// The active SQL buffer/cursor (edtui).
    pub editor: edtui::EditorState,
    /// The shared key handler (vim + readline insert chords).
    pub handler: edtui::EditorEventHandler,
    /// The context picker child sub-module.
    pub context_picker: ContextPickerState,
    /// The SQL completion child sub-module.
    pub sql_completion: SqlCompletionState,
    /// In-buffer `/` search state (query + live matches).
    pub sql_search: EditorSqlSearch,
    /// The cached SQL-completion catalog for the tab's connection/schema.
    pub completion_catalog: CompletionCatalog,
}

impl std::fmt::Debug for EditorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorState")
            .field("sql", &crate::common::editor::editor_text(&self.editor))
            .field("cursor", &self.editor.cursor)
            .field("mode", &self.editor.mode)
            .field("context_picker", &self.context_picker)
            .field("sql_completion", &self.sql_completion)
            .field("sql_search", &self.sql_search)
            .finish()
    }
}

impl Default for EditorState {
    fn default() -> Self {
        EditorState {
            editor: editor::new_editor(""),
            handler: editor::new_editor_handler(),
            context_picker: ContextPickerState::default(),
            sql_completion: SqlCompletionState::default(),
            sql_search: EditorSqlSearch::default(),
            completion_catalog: CompletionCatalog::default(),
        }
    }
}

impl EditorState {
    /// Build an editor seeded with `sql` text.
    pub fn with_sql(sql: &str) -> Self {
        EditorState {
            editor: editor::new_editor(sql),
            handler: editor::new_editor_handler(),
            context_picker: ContextPickerState::default(),
            sql_completion: SqlCompletionState::default(),
            sql_search: EditorSqlSearch::default(),
            completion_catalog: CompletionCatalog::default(),
        }
    }
}
