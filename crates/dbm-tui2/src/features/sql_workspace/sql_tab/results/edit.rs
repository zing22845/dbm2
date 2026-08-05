//! Edit-session state for Results row editing (pure).
//!
//! Tracks the loaded-row snapshots, per-cell overrides, deleted rows, pending
//! inserts and conflict rows, plus a cached commit row count. All mutators are
//! pure so the session can be unit-tested without a DB.

use std::collections::{HashMap, HashSet};

use crate::common::model::RowChangeKind;

/// Classify a row's change kind in the current edit session.
pub fn row_change_kind(state: &ResultsEditState, row: usize) -> RowChangeKind {
    if !state.editing {
        return RowChangeKind::NoChange;
    }
    let loaded = state.snapshots.len();
    if row < loaded {
        if state.deleted.contains(&row) {
            return RowChangeKind::Delete;
        }
        if state.dirty_cells.keys().any(|(r, _)| *r == row) {
            return RowChangeKind::Update;
        }
        return RowChangeKind::NoChange;
    }
    let new_idx = row - loaded;
    if new_idx < state.new_rows.len() {
        RowChangeKind::Insert
    } else {
        RowChangeKind::NoChange
    }
}

pub fn cell_is_dirty(state: &ResultsEditState, row: usize, col: usize) -> bool {
    state.editing && state.dirty_cells.contains_key(&(row, col))
}

pub fn gutter_glyph(kind: RowChangeKind) -> char {
    match kind {
        RowChangeKind::NoChange => ' ',
        RowChangeKind::Insert => '+',
        RowChangeKind::Delete => '-',
        RowChangeKind::Update => '~',
    }
}

#[derive(Debug, Clone, Default)]
pub struct ResultsEditState {
    pub editing: bool,
    /// Snapshot of loaded result rows when Edit mode was entered.
    pub snapshots: Vec<Vec<String>>,
    /// Cell overrides keyed by (row_in_loaded_rows, col).
    pub dirty_cells: HashMap<(usize, usize), String>,
    pub deleted: HashSet<usize>,
    pub new_rows: Vec<Vec<String>>,
    pub conflict_rows: HashSet<usize>,
    /// Cached result of `commit_row_count`, kept in sync by the mutators.
    pub commit_row_count_cache: usize,
}

impl ResultsEditState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Recompute `commit_row_count_cache` from the current dirty/deleted/new
    /// state. Called by every mutator so the cached count stays exact.
    fn recompute_commit_count(&mut self) {
        let mut rows: HashSet<usize> = HashSet::new();
        for (r, _) in self.dirty_cells.keys() {
            rows.insert(*r);
        }
        rows.extend(self.deleted.iter().copied());
        self.commit_row_count_cache = rows.len() + self.new_rows.len();
    }

    pub fn enter_edit(&mut self, rows: &[Vec<String>]) {
        self.editing = true;
        self.snapshots = rows.to_vec();
        self.dirty_cells.clear();
        self.deleted.clear();
        self.new_rows.clear();
        self.conflict_rows.clear();
        self.recompute_commit_count();
    }

    pub fn rollback(&mut self) {
        let snapshots = self.snapshots.clone();
        self.dirty_cells.clear();
        self.deleted.clear();
        self.new_rows.clear();
        self.conflict_rows.clear();
        self.snapshots = snapshots;
        self.recompute_commit_count();
        // stays in editing mode with a clean buffer
    }

    pub fn exit_edit(&mut self) {
        self.clear();
    }

    pub fn is_dirty(&self) -> bool {
        commit_row_count(self) > 0
    }

    pub fn apply_cell(&mut self, row: usize, col: usize, value: String) {
        if !self.editing || self.deleted.contains(&row) {
            return;
        }
        let loaded = self.snapshots.len();
        if row >= loaded {
            let Some(new_row) = self.new_rows.get_mut(row - loaded) else {
                return;
            };
            if let Some(cell) = new_row.get_mut(col) {
                *cell = value;
            }
            return;
        }
        let Some(snap) = self.snapshots.get(row) else {
            return;
        };
        if snap.get(col).is_some_and(|old| old == &value) {
            self.dirty_cells.remove(&(row, col));
        } else {
            self.dirty_cells.insert((row, col), value);
        }
        self.conflict_rows.remove(&row);
        self.recompute_commit_count();
    }

    pub fn insert_row(&mut self, col_count: usize) {
        if !self.editing {
            return;
        }
        self.new_rows.push(vec![String::new(); col_count]);
        self.recompute_commit_count();
    }

    /// Append a pending insert row prefilled with `values` (e.g. duplicate).
    pub fn insert_row_values(&mut self, values: Vec<String>) {
        if !self.editing {
            return;
        }
        self.new_rows.push(values);
        self.recompute_commit_count();
    }

    /// Returns `true` when a pending insert row was removed from `new_rows`.
    pub fn mark_delete(&mut self, row: usize) -> bool {
        if !self.editing {
            return false;
        }
        let loaded = self.snapshots.len();
        if row >= loaded {
            let idx = row - loaded;
            if idx < self.new_rows.len() {
                self.new_rows.remove(idx);
                self.recompute_commit_count();
                return true;
            }
            return false;
        }
        // Toggle delete mark.
        if !self.deleted.insert(row) {
            self.deleted.remove(&row);
        } else {
            // Clear cell dirties for the deleted row.
            self.dirty_cells.retain(|&(r, _), _| r != row);
        }
        self.conflict_rows.remove(&row);
        self.recompute_commit_count();
        false
    }
}

/// Distinct rows that participate in Commit (modified ∪ deleted ∪ new).
pub fn commit_row_count(state: &ResultsEditState) -> usize {
    state.commit_row_count_cache
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_row_count_counts_distinct_rows() {
        let mut s = ResultsEditState::default();
        s.enter_edit(&[vec!["1".into(), "a".into()]]);
        s.apply_cell(0, 1, "b".into());
        s.mark_delete(0);
        assert_eq!(commit_row_count(&s), 1);
        s.insert_row(2);
        assert_eq!(commit_row_count(&s), 2);
    }

    #[test]
    fn apply_cell_clears_when_restored_to_snapshot() {
        let mut s = ResultsEditState::default();
        s.enter_edit(&[vec!["1".into(), "a".into()]]);
        s.apply_cell(0, 1, "b".into());
        assert!(s.is_dirty());
        s.apply_cell(0, 1, "a".into());
        assert!(!s.is_dirty());
    }

    #[test]
    fn row_change_kind_insert_delete_update() {
        let mut s = ResultsEditState::default();
        s.enter_edit(&[vec!["1".into(), "a".into()]]);
        assert_eq!(row_change_kind(&s, 0), RowChangeKind::NoChange);
        s.apply_cell(0, 1, "b".into());
        assert_eq!(row_change_kind(&s, 0), RowChangeKind::Update);
        assert!(cell_is_dirty(&s, 0, 1));
        assert!(!cell_is_dirty(&s, 0, 0));
        s.mark_delete(0);
        assert_eq!(row_change_kind(&s, 0), RowChangeKind::Delete);
        s.rollback();
        s.insert_row(2);
        assert_eq!(row_change_kind(&s, 1), RowChangeKind::Insert);
        assert_eq!(gutter_glyph(RowChangeKind::Insert), '+');
    }

    #[test]
    fn apply_cell_on_new_row_updates_new_rows() {
        let mut s = ResultsEditState::default();
        s.enter_edit(&[vec!["1".into()]]);
        s.insert_row(1);
        s.apply_cell(1, 0, "x".into());
        assert_eq!(s.new_rows[0][0], "x");
        assert_eq!(row_change_kind(&s, 1), RowChangeKind::Insert);
    }

    #[test]
    fn mark_delete_removes_new_row() {
        let mut s = ResultsEditState::default();
        s.enter_edit(&[vec!["1".into()]]);
        s.insert_row(1);
        assert!(s.mark_delete(1));
        assert!(s.new_rows.is_empty());
    }

    #[test]
    fn insert_row_values_prefills_new_row() {
        let mut s = ResultsEditState::default();
        s.enter_edit(&[vec!["1".into(), "a".into()]]);
        let values = vec!["1".to_string(), "a".to_string()];
        s.insert_row_values(values.clone());
        assert_eq!(s.new_rows, vec![values]);
        assert_eq!(row_change_kind(&s, 1), RowChangeKind::Insert);
    }
}
