//! Results feature state.
//!
//! Owns the last query result, the cell selection (row/col), the `/` search,
//! pagination (page / row limit / last-page flag), and the detail sub-pane.

use crate::common::components::search::PaneSearch;

use super::detail::state::DetailState;
use super::edit::ResultsEditState;
use super::edit_sql::EditTarget;
use super::pagination::{DEFAULT_RESULTS_ROW_LIMIT, can_go_next};

/// An `Eq` projection of `dbm_core::QueryResult` so it can travel through the
/// (Eq-based) message router. `ColumnInfo` is the `Eq` column metadata used by
/// the completion engine; rows are plain `String` cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryResultData {
    pub columns: Vec<super::super::editor::sql_completion::provider::ColumnInfo>,
    pub rows: Vec<Vec<String>>,
    pub rows_affected: Option<u64>,
    pub total_rows: Option<u64>,
}

impl From<dbm_core::QueryResult> for QueryResultData {
    fn from(q: dbm_core::QueryResult) -> Self {
        QueryResultData {
            columns: q.columns.into_iter().map(Into::into).collect(),
            rows: q.rows.into_iter().map(|r| r.values).collect(),
            rows_affected: q.rows_affected,
            total_rows: q.total_rows,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ResultsState {
    /// The last query result (None when no query has run / it failed).
    pub result: Option<QueryResultData>,
    /// Selected cell (row, col) into `result`.
    pub row: usize,
    pub col: usize,
    /// The `/` search state.
    pub search: PaneSearch,
    /// Current 1-based page.
    pub page: usize,
    /// Rows per page.
    pub row_limit: usize,
    /// Whether we've reached the last SQL page.
    pub at_last_page: bool,
    /// Whether the result came from a paginated query.
    pub paginated: bool,
    /// Horizontal scroll offset of the result table.
    pub h_scroll: usize,
    /// The detail sub-pane state.
    pub detail: DetailState,
    /// The row-edit session (snapshots / dirty cells / deleted / new rows).
    pub edit: ResultsEditState,
    /// The resolved edit target (schema/table/primary keys) when editable.
    pub edit_target: Option<EditTarget>,
    /// The detail draft's baseline (cell value at load) for dirty detection.
    pub detail_baseline: String,
    /// The detail draft's current text (edited value).
    pub detail_draft: String,
    /// Whether the detail draft is dirty (differs from baseline).
    pub detail_dirty: bool,
    /// Whether an unsaved detail draft blocks leaving Detail.
    pub detail_leave_warning: bool,
}

impl ResultsState {
    /// Build a fresh state with default pagination.
    pub fn new() -> Self {
        ResultsState {
            page: 1,
            row_limit: DEFAULT_RESULTS_ROW_LIMIT,
            ..ResultsState::default()
        }
    }

    /// The value at the selected cell, if any.
    pub fn selected_cell(&self) -> Option<String> {
        let result = self.result.as_ref()?;
        result.rows.get(self.row)?.get(self.col).cloned()
    }

    /// The column name at the selected column, if any.
    pub fn selected_column_name(&self) -> Option<&str> {
        self.result
            .as_ref()
            .and_then(|r| r.columns.get(self.col))
            .map(|meta| meta.name.as_str())
    }

    /// Total display rows (rows in the current result).
    pub fn row_count(&self) -> usize {
        self.result.as_ref().map(|r| r.rows.len()).unwrap_or(0)
    }

    /// Whether the selection can move further to the next SQL page.
    pub fn can_go_next_page(&self) -> bool {
        if !self.paginated {
            return false;
        }
        can_go_next(
            self.page,
            self.row_limit,
            self.row_count(),
            self.result.as_ref().and_then(|r| r.total_rows),
            self.at_last_page,
        )
    }

    /// Move the cell selection by `(dr, dc)`, clamped to the result.
    pub fn move_selection(&mut self, dr: i32, dc: i32) -> bool {
        let row_count = self.row_count();
        let Some(col_count) = self.result.as_ref().map(|r| r.columns.len()) else {
            return false;
        };
        if row_count == 0 {
            return false;
        }
        let prev_row = self.row;
        let prev_col = self.col;
        if dr != 0 {
            self.row = if dr > 0 {
                (self.row + 1).min(row_count - 1)
            } else {
                self.row.saturating_sub(1)
            };
        }
        if dc != 0 {
            self.col = if dc > 0 {
                (self.col + 1).min(col_count.saturating_sub(1))
            } else {
                self.col.saturating_sub(1)
            };
        }
        self.row != prev_row || self.col != prev_col
    }

    // ---- Edit session (row editing / save flow) ----

    /// Enter edit mode with the current result rows as snapshots.
    pub fn enter_edit(&mut self) {
        if !self.editable() {
            return;
        }
        let rows: Vec<Vec<String>> = self
            .result
            .as_ref()
            .map(|r| r.rows.clone())
            .unwrap_or_default();
        self.edit.enter_edit(&rows);
    }

    /// Whether the current result is editable (has an edit target).
    pub fn editable(&self) -> bool {
        self.edit_target.is_some()
    }

    /// Roll back all edits, restoring snapshots into the result rows.
    pub fn rollback_edits(&mut self) {
        self.edit.rollback();
        if let Some(result) = self.result.as_mut() {
            result.rows = self.edit.snapshots.clone();
        }
        self.detail_baseline.clear();
        self.detail_draft.clear();
        self.detail_dirty = false;
        self.detail_leave_warning = false;
    }

    /// Exit edit mode (clears the session).
    pub fn exit_edit(&mut self) {
        self.edit.exit_edit();
    }

    /// Apply an edited cell value into the edit session (and the live result).
    pub fn apply_cell_value(&mut self, row: usize, col: usize, value: String) {
        self.edit.apply_cell(row, col, value.clone());
        if let Some(result) = self.result.as_mut() {
            if row < result.rows.len()
                && let Some(cell) = result.rows.get_mut(row).and_then(|r| r.get_mut(col))
            {
                *cell = value;
            }
        }
    }

    /// Add an empty row at the end (pending insert).
    pub fn edit_add_row(&mut self) {
        if !self.edit.editing {
            return;
        }
        let cols = self.result.as_ref().map(|r| r.columns.len()).unwrap_or(0);
        self.edit.insert_row(cols);
        self.append_row(vec![String::new(); cols]);
    }

    /// Duplicate the selected row as a pending insert.
    pub fn edit_dup_row(&mut self) {
        if !self.edit.editing {
            return;
        }
        let Some(values) = self.result.as_ref().and_then(|r| r.rows.get(self.row)).cloned() else {
            return;
        };
        if values.is_empty() {
            return;
        }
        self.edit.insert_row_values(values.clone());
        self.append_row(values);
    }

    /// Delete the selected row (toggles the delete mark; removes pending inserts).
    pub fn edit_del_row(&mut self) {
        if !self.edit.editing {
            return;
        }
        let removed_insert = self.edit.mark_delete(self.row);
        if removed_insert
            && let Some(result) = self.result.as_mut()
            && self.row < result.rows.len()
        {
            result.rows.remove(self.row);
        }
    }

    /// Append a row to the live result and select it.
    fn append_row(&mut self, values: Vec<String>) {
        if let Some(result) = self.result.as_mut() {
            result.rows.push(values);
            self.row = result.rows.len().saturating_sub(1);
            self.col = 0;
        }
    }

    /// Build the ordered commit DML from the current edit session.
    pub fn build_commit_statements(&self) -> Result<Vec<String>, String> {
        let Some(target) = self.edit_target.as_ref() else {
            return Err("not editable".into());
        };
        super::edit_sql::build_commit_statements(target, &self.edit)
    }

    /// Commit-row count for the current edit session.
    pub fn commit_row_count(&self) -> usize {
        super::edit::commit_row_count(&self.edit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;

    fn col(name: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_name: "text".into(),
            type_display: "text".into(),
            comment: None,
        }
    }

    fn sample() -> QueryResultData {
        QueryResultData {
            columns: vec![col("id"), col("name")],
            rows: vec![
                vec!["1".into(), "alice".into()],
                vec!["2".into(), "bob".into()],
            ],
            rows_affected: None,
            total_rows: None,
        }
    }

    #[test]
    fn move_selection_clamps_to_bounds() {
        let mut s = ResultsState {
            result: Some(sample()),
            ..ResultsState::default()
        };
        assert!(s.move_selection(1, 0));
        assert_eq!(s.row, 1);
        // At the last row, moving down is a no-op.
        assert!(!s.move_selection(1, 0));
        assert_eq!(s.row, 1);
        // Column stays within bounds.
        assert!(s.move_selection(0, 1));
        assert_eq!(s.col, 1);
        assert!(!s.move_selection(0, 1));
        assert_eq!(s.col, 1);
    }

    #[test]
    fn selected_cell_returns_value() {
        let s = ResultsState {
            result: Some(sample()),
            col: 1,
            row: 1,
            ..ResultsState::default()
        };
        assert_eq!(s.selected_cell().as_deref(), Some("bob"));
        assert_eq!(s.selected_column_name(), Some("name"));
    }

    #[test]
    fn can_go_next_page_respects_pagination_and_total() {
        let mut s = ResultsState {
            result: Some(QueryResultData {
                rows: vec![vec!["1".into()]; 100],
                columns: vec![col("x")],
                rows_affected: None,
                total_rows: Some(300),
            }),
            paginated: true,
            row_limit: 100,
            page: 2,
            ..ResultsState::default()
        };
        assert!(s.can_go_next_page());
        s.page = 3;
        assert!(!s.can_go_next_page());
        // Non-paginated results cannot page.
        s.paginated = false;
        assert!(!s.can_go_next_page());
    }

    #[test]
    fn query_result_data_from_dbm_converts_rows() {
        let q = dbm_core::QueryResult {
            columns: vec![dbm_core::ColumnMeta {
                name: "id".into(),
                type_name: "int4".into(),
                type_display: "int4".into(),
                comment: Some("pk".into()),
            }],
            rows: vec![dbm_core::Row {
                values: vec!["7".into()],
            }],
            rows_affected: None,
            total_rows: Some(1),
        };
        let d: QueryResultData = q.into();
        assert_eq!(d.rows[0][0], "7");
        assert_eq!(d.columns[0].name, "id");
        assert_eq!(d.columns[0].comment.as_deref(), Some("pk"));
        assert_eq!(d.total_rows, Some(1));
    }

    #[test]
    fn enter_edit_snapshots_rows_and_applies_cells() {
        let mut s = ResultsState {
            result: Some(sample()),
            edit_target: Some(super::super::edit_sql::EditTarget {
                schema: "public".into(),
                table: "users".into(),
                primary_keys: vec!["id".into()],
                columns: vec!["id".into(), "name".into()],
            }),
            ..ResultsState::default()
        };
        s.enter_edit();
        assert!(s.edit.editing);
        s.col = 1;
        s.apply_cell_value(0, 1, "carol".into());
        assert!(s.edit.is_dirty());
        assert_eq!(s.selected_cell().as_deref(), Some("carol"));
        assert_eq!(s.commit_row_count(), 1);
    }

    #[test]
    fn edit_add_and_dup_and_del_rows() {
        let mut s = ResultsState {
            result: Some(sample()),
            edit_target: Some(super::super::edit_sql::EditTarget {
                schema: "public".into(),
                table: "users".into(),
                primary_keys: vec!["id".into()],
                columns: vec!["id".into(), "name".into()],
            }),
            ..ResultsState::default()
        };
        s.enter_edit();
        s.edit_add_row();
        assert_eq!(s.result.as_ref().unwrap().rows.len(), 3);
        assert_eq!(s.commit_row_count(), 1);
        // Dup the selected row (still row 0 after add).
        s.row = 0;
        s.edit_dup_row();
        assert_eq!(s.result.as_ref().unwrap().rows.len(), 4);
        // Delete a pending insert (last row).
        s.row = s.result.as_ref().unwrap().rows.len() - 1;
        s.edit_del_row();
        assert_eq!(s.result.as_ref().unwrap().rows.len(), 3);
    }

    #[test]
    fn build_commit_statements_from_edit_session() {
        let mut s = ResultsState {
            result: Some(sample()),
            edit_target: Some(super::super::edit_sql::EditTarget {
                schema: "public".into(),
                table: "users".into(),
                primary_keys: vec!["id".into()],
                columns: vec!["id".into(), "name".into()],
            }),
            ..ResultsState::default()
        };
        s.enter_edit();
        s.apply_cell_value(0, 1, "dave".into());
        let stmts = s.build_commit_statements().unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].contains("UPDATE"));
        assert!(stmts[0].contains("\"public\".\"users\""));
        assert!(stmts[0].contains("'dave'"));
    }

    #[test]
    fn rollback_edits_restores_snapshots_and_clears_draft() {
        let mut s = ResultsState {
            result: Some(sample()),
            edit_target: Some(super::super::edit_sql::EditTarget {
                schema: "public".into(),
                table: "users".into(),
                primary_keys: vec!["id".into()],
                columns: vec!["id".into(), "name".into()],
            }),
            ..ResultsState::default()
        };
        s.enter_edit();
        s.apply_cell_value(0, 1, "zoe".into());
        assert!(s.edit.is_dirty());
        s.detail_draft = "zoe".into();
        s.detail_dirty = true;
        s.rollback_edits();
        assert!(!s.edit.is_dirty());
        assert_eq!(s.result.as_ref().unwrap().rows[0][1], "alice");
        assert!(!s.detail_dirty);
    }
}

