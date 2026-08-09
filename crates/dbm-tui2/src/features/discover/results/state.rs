//! Discovery results feature state.

/// State for the discovery results list.
#[derive(Debug, Clone)]
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

impl Default for ResultsState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            cursor: 0,
            scroll: 0,
            // Default to hiding already-registered instances: discover is meant
            // to surface *new* hosts, and registered instances cannot be
            // registered again (they would hit ALREADY_REGISTERED). The user can
            // toggle this off with `u` to review the full list.
            unregistered_only: true,
            selected: Vec::new(),
        }
    }
}

impl ResultsState {
    /// Whether there are any results at all.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The `items` indices that are visible under the active filter, in display
    /// order. When `unregistered_only` is set, already-registered instances are
    /// hidden (they cannot be registered again).
    pub fn visible_indices(&self) -> Vec<usize> {
        if self.unregistered_only {
            self.items
                .iter()
                .enumerate()
                .filter(|(_, i)| !i.already_registered)
                .map(|(i, _)| i)
                .collect()
        } else {
            (0..self.items.len()).collect()
        }
    }

    /// Number of rows in the current filtered view.
    pub fn row_count(&self) -> usize {
        self.visible_indices().len()
    }

    /// Replace the results with a new scan's instances and reset the cursor.
    pub fn set_items(&mut self, items: Vec<dbm_discovery::DiscoveredInstance>) {
        self.items = items;
        self.cursor = 0;
        self.scroll = 0;
        self.selected.clear();
    }

    /// Toggle whether only unregistered instances are shown. Clamps the cursor
    /// back into the (possibly smaller) visible range. Returns whether anything
    /// changed; a no-op when there is nothing to show.
    pub fn toggle_unregistered_filter(&mut self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        self.unregistered_only = !self.unregistered_only;
        let rows = self.row_count();
        self.cursor = self.cursor.min(rows.saturating_sub(1));
        self.scroll = self.scroll.min(rows.saturating_sub(1));
        true
    }

    /// Move the cursor up (clamped). Returns whether it moved.
    pub fn move_up(&mut self) -> bool {
        let before = self.cursor;
        self.cursor = self.cursor.saturating_sub(1);
        self.cursor != before
    }

    /// Move the cursor down (clamped). Returns whether it moved.
    pub fn move_down(&mut self) -> bool {
        let rows = self.row_count();
        if rows == 0 {
            return false;
        }
        let before = self.cursor;
        self.cursor = (self.cursor + 1).min(rows - 1);
        self.cursor != before
    }

    /// Toggle selection of the row under the cursor. The cursor is a position
    /// within the *visible* list; the underlying `items` index is stored so
    /// `selected_discovery_ids` stays stable when the filter changes. Returns
    /// whether the selection actually changed; a no-op when the list is empty.
    ///
    /// An already-registered row always renders the same `×` mark regardless of
    /// selection, so toggling it reports `false` (no repaint) — otherwise a held
    /// SPACE would redraw on every auto-repeat even though nothing on screen
    /// changes, matching the original dbm's behavior.
    pub fn toggle_select(&mut self) -> bool {
        let Some(item_idx) = self.item_index_at(self.cursor) else {
            return false;
        };
        let already_registered = self
            .items
            .get(item_idx)
            .is_some_and(|item| item.already_registered);
        if let Some(pos) = self.selected.iter().position(|&i| i == item_idx) {
            self.selected.remove(pos);
        } else {
            self.selected.push(item_idx);
        }
        !already_registered
    }

    /// Discovery ids of the currently selected rows.
    pub fn selected_discovery_ids(&self) -> Vec<String> {
        self.selected
            .iter()
            .filter_map(|&i| self.items.get(i))
            .map(|item| item.discovery_id.clone())
            .collect()
    }

    /// The `items` index at the given visible position, if any.
    fn item_index_at(&self, vis: usize) -> Option<usize> {
        self.visible_indices().get(vis).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbm_discovery::{
        Confidence, DiscoveredInstance, DiscoverySource, InstanceRunStatus,
    };

    fn instance(id: &str, registered: bool) -> DiscoveredInstance {
        DiscoveredInstance {
            discovery_id: id.to_string(),
            fingerprint: id.to_string(),
            engine: dbm_core::Engine::Postgres,
            host: "127.0.0.1".to_string(),
            port: 5432,
            socket_path: None,
            data_dir: None,
            systemd_unit: None,
            version: None,
            status: InstanceRunStatus::Running,
            sources: vec![DiscoverySource::Port],
            confidence: Confidence::High,
            already_registered: registered,
            registered_instance_id: registered.then(|| "inst-1".to_string()),
            scanned_at: String::new(),
        }
    }

    #[test]
    fn toggle_select_registered_row_does_not_repaint() {
        // Show all rows (including already-registered ones) so the cursor can
        // land on a registered row.
        let mut state = ResultsState {
            items: vec![
                instance("a", true),  // registered -> always renders `×`
                instance("b", false), // unregistered
            ],
            unregistered_only: false,
            ..ResultsState::default()
        };
        // Cursor starts at row 0 = the registered instance.
        assert!(!state.toggle_select(), "registered row mark is unchanged");
        // Its selection state still toggled, even though we do not repaint.
        assert!(state.selected.contains(&0));
        // Toggling again also reports no repaint.
        assert!(!state.toggle_select());
        assert!(state.selected.is_empty());
    }

    #[test]
    fn toggle_select_unregistered_row_repaints() {
        let mut state = ResultsState {
            items: vec![instance("b", false)],
            unregistered_only: false,
            ..ResultsState::default()
        };
        // An unregistered row switches between blank and `✓`, so it repaints.
        assert!(state.toggle_select());
        assert!(state.selected.contains(&0));
        assert!(state.toggle_select());
        assert!(state.selected.is_empty());
    }
}
