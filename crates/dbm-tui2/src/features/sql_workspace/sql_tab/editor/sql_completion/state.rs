//! SQL completion sub-module state.
//!
//! Holds the completion popup's open flag, the ranked items and the selected
//! index, plus the replace range (in cursor coordinates) that a completion
//! application will overwrite. The engine is pure and fed through `Refresh`;
//! the actual buffer edit is deferred to the editor (an `Apply` intent).

use crate::common::utils::cursor::Cursor;

use super::provider::CompletionItem;

#[derive(Debug, Clone, Default)]
pub struct SqlCompletionState {
    /// Whether the completion popup is open.
    pub open: bool,
    /// The ranked completion items.
    pub items: Vec<CompletionItem>,
    /// Index of the currently selected item.
    pub selected: usize,
    /// Start of the range the applied completion replaces.
    pub replace_start: Cursor,
    /// End of the range the applied completion replaces.
    pub replace_end: Cursor,
}

impl SqlCompletionState {
    /// Open the popup with the given items and replace range.
    pub fn open_with(
        items: Vec<CompletionItem>,
        replace_start: Cursor,
        replace_end: Cursor,
    ) -> Self {
        SqlCompletionState {
            open: !items.is_empty(),
            items,
            selected: 0,
            replace_start,
            replace_end,
        }
    }

    /// Close the popup and clear its items.
    pub fn close(&mut self) {
        self.open = false;
        self.items.clear();
        self.selected = 0;
    }

    /// Whether the popup is open with at least one item.
    pub fn is_open(&self) -> bool {
        self.open && !self.items.is_empty()
    }

    /// The currently selected item, if any.
    pub fn selected_item(&self) -> Option<&CompletionItem> {
        if !self.open {
            return None;
        }
        self.items.get(self.selected)
    }

    /// Move the selection by `delta`, wrapping around.
    pub fn move_selection(&mut self, delta: i32) {
        if self.items.is_empty() {
            return;
        }
        let len = self.items.len() as i32;
        let next = (self.selected as i32 + delta).rem_euclid(len);
        self.selected = next as usize;
    }
}
