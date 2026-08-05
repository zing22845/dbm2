//! Editor feature state.

use super::context_picker::state::ContextPickerState;
use super::sql_completion::state::SqlCompletionState;

#[derive(Debug, Default, Clone)]
pub struct EditorState {
    pub context_picker: ContextPickerState,
    pub sql_completion: SqlCompletionState,
}
