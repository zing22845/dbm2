//! Discovery results feature state.

/// State for the discovery results list.
#[derive(Debug, Clone, Default)]
pub struct ResultsState {
    /// The discovered instances (populated by the scan effect).
    pub items: Vec<dbm_discovery::DiscoveredInstance>,
    /// Cursor row within the (filtered) results.
    pub cursor: usize,
    /// Scroll offset of the results list.
    pub scroll: usize,
    /// Whether only unregistered instances are shown.
    pub unregistered_only: bool,
    /// Indices of the currently selected results.
    pub selected: Vec<usize>,
}

impl ResultsState {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Number of rows in the current filtered view.
    pub fn row_count(&self) -> usize {
        self.items.len()
    }

    /// Replace the results with a new scan's instances and reset the cursor.
    pub fn set_items(&mut self, items: Vec<dbm_discovery::DiscoveredInstance>) {
        self.items = items;
        self.cursor = 0;
        self.scroll = 0;
        self.selected.clear();
    }

    /// Move the cursor up (clamped). Returns whether it moved.
    pub fn move_up(&mut self) -> bool {
        let before = self.cursor;
        self.cursor = self.cursor.saturating_sub(1);
        self.cursor != before
    }

    /// Move the cursor down (clamped). Returns whether it moved.
    pub fn move_down(&mut self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        let before = self.cursor;
        self.cursor = (self.cursor + 1).min(self.items.len() - 1);
        self.cursor != before
    }

    /// Toggle selection of the row under the cursor.
    pub fn toggle_select(&mut self) {
        if let Some(pos) = self.selected.iter().position(|&i| i == self.cursor) {
            self.selected.remove(pos);
        } else {
            self.selected.push(self.cursor);
        }
    }

    /// Discovery ids of the currently selected rows.
    pub fn selected_discovery_ids(&self) -> Vec<String> {
        self.selected
            .iter()
            .filter_map(|&i| self.items.get(i))
            .map(|item| item.discovery_id.clone())
            .collect()
    }
}
