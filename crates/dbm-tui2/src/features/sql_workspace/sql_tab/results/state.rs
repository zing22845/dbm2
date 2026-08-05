//! Results feature state.
//!
//! Owns the last query result, the cell selection (row/col), the `/` search,
//! pagination (page / row limit / last-page flag), and the detail sub-pane.

use crate::common::components::search::PaneSearch;

use super::detail::state::DetailState;
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
}

