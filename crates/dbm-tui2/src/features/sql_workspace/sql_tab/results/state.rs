//! Results feature state.
//!
//! Composes the `list`, `detail`, and `splitter` sub-feature states plus the
//! parent-level `detail_open` flag that controls horizontal layout splitting.

use super::detail::state::DetailState;
use super::list::state::ListState;
use super::splitter::state::SplitterState;

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
    /// The list sub-feature state (result, selection, pagination, edit).
    pub list: ListState,
    /// The detail sub-feature state (scroll, draft).
    pub detail: DetailState,
    /// The internal list/detail splitter state (detail pane width).
    pub splitter: SplitterState,
    /// Whether the detail sub-pane is open (inspect mode). When open, the
    /// results content area splits horizontally into list + detail panes.
    pub detail_open: bool,
}

impl ResultsState {
    /// Build a fresh state with default pagination.
    pub fn new() -> Self {
        ResultsState {
            list: ListState::new(),
            ..ResultsState::default()
        }
    }

    /// Open the detail sub-pane (enter inspect mode).
    pub fn open_detail(&mut self) {
        self.detail_open = true;
        self.detail.reset_for_close();
    }

    /// Close the detail sub-pane (exit inspect mode).
    pub fn close_detail(&mut self) {
        self.detail_open = false;
        self.detail.reset_for_close();
    }

    /// Toggle the detail sub-pane open/close.
    pub fn toggle_detail(&mut self) {
        if self.detail_open {
            self.close_detail();
        } else {
            self.open_detail();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_state_default_has_empty_list_and_detail() {
        let s = ResultsState::default();
        assert!(s.list.result.is_none());
        assert!(!s.detail_open);
        assert_eq!(s.detail.scroll, 0);
    }

    #[test]
    fn open_close_detail_toggles_flag() {
        let mut s = ResultsState::default();
        assert!(!s.detail_open);
        s.open_detail();
        assert!(s.detail_open);
        s.close_detail();
        assert!(!s.detail_open);
    }
}
