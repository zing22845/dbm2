//! Results list sub-module state.
//!
//! Owns the query result, cell selection, search, pagination, and edit session.

use std::cell::Cell;

use crate::common::components::search::PaneSearch;
use crate::common::view::format::init_results_layout;

use super::super::edit::ResultsEditState;
use super::super::edit_sql::EditTarget;
use super::super::pagination::{DEFAULT_RESULTS_ROW_LIMIT, ResultsPageAction, can_go_next};
use super::super::state::QueryResultData;
use super::search::ResultsSearchMatch;

/// State for the results list sub-feature.
#[derive(Debug, Default, Clone)]
pub struct ListState {
    /// The last query result (None when no query has run / it failed).
    pub result: Option<QueryResultData>,
    /// The last query failure message.
    pub query_error: Option<String>,
    /// Selected cell (row, col) into `result`.
    pub row: usize,
    pub col: usize,
    /// Whether a cell is actively selected. When `false` (deselected via Esc)
    /// no cell cursor is shown and a newly started search matches all columns.
    pub selected: bool,
    /// The `/` search state.
    pub search: PaneSearch,
    /// The column scoping the search, when limited to one column (`None` = all).
    pub search_scope_column: Option<usize>,
    /// All current-match occurrences (cached from the last search refresh).
    pub search_matches: Vec<ResultsSearchMatch>,
    /// Index into `search_matches` of the active match.
    pub search_match_index: usize,
    /// Current 1-based page.
    pub page: usize,
    /// Rows per page.
    pub row_limit: usize,
    /// Whether we've reached the last SQL page.
    pub at_last_page: bool,
    /// Whether the result came from a paginated query.
    pub paginated: bool,
    /// Transient cursor anchor for the next landed result: when a page-nav
    /// moved *backwards* (Prev / Last) the incoming page's cursor is placed on
    /// its last row instead of the top, mirroring the original dbm's
    /// "previous page lands at the bottom". Cleared once the result lands.
    pub page_anchor_bottom: bool,
    /// Transient `<` / `>` double-press chord state (`forward`, armed-at):
    /// a second press of the same key within the chord window upgrades to
    /// first / last page. Any non-chord message clears it.
    pub page_chord: Option<(bool, std::time::Instant)>,
    /// When the last `d` (delete-row) press was made while editing, for the
    /// original dbm's `dd` chord: a second `d` within 500 ms deletes the
    /// selected row, a lone `d` does nothing.
    pub del_chord_at: Option<std::time::Instant>,
    /// A COUNT(*) total-rows request is in flight (`[c]counting…`).
    pub counting: bool,
    /// A page action queued until the in-flight count lands (the original dbm
    /// counts first when a "last page" jump hits an unknown total).
    pub pending_page_after_count: Option<ResultsPageAction>,
    /// The known grand-total row count carried across paginated re-runs. Page
    /// fetches themselves return no total (lazy count, like the original dbm);
    /// once the user counts, this cache keeps the "page x/N" toolbar alive
    /// while browsing further pages of the same query. Cleared when a new
    /// query starts.
    pub total_cache: Option<u64>,
    /// Horizontal scroll offset of the result table.
    /// Uses `Cell` for interior mutability: the view writes the computed
    /// anchored scroll back here so the next frame starts from the correct
    /// position (fixes the stale h_scroll issue).
    pub h_scroll: Cell<usize>,
    /// Vertical scroll offset (index of first visible data row).
    pub v_scroll: Cell<usize>,
    /// Number of data rows that fit in the current viewport.
    pub viewport_rows: Cell<usize>,
    /// Width of the visible table area (columns viewport) in characters.
    pub viewport_width: Cell<u16>,
    /// The row-edit session (snapshots / dirty cells / deleted / new rows).
    pub edit: ResultsEditState,
    /// The SQL text of the last run query (kept for editability analysis).
    pub last_sql: String,
    /// The connection context of the last run query.
    pub last_instance: String,
    pub last_connection: String,
    pub last_database: Option<String>,
    pub last_schema: String,
    /// The resolved edit target when editable.
    pub edit_target: Option<EditTarget>,
    /// Why the current result cannot be edited, if it cannot.
    pub edit_blocked_reason: Option<String>,
    /// Auto-sized column widths computed from the current result.
    pub col_widths: Vec<u16>,
    /// When true, the viewport anchor that keeps the cursor visible is
    /// suppressed so scrollbar drags / manual scrolls are not overridden.
    /// Cleared on any cursor-movement message.
    pub scroll_locked: Cell<bool>,
    /// Whether a leave attempt was blocked by unsaved edits (a dirty edit
    /// session). Cleared when the edits are rolled back / committed or the
    /// session exits, and drives the red footer warning.
    pub leave_warning: bool,
}

impl ListState {
    /// Build a fresh state with default pagination.
    pub fn new() -> Self {
        ListState {
            // A cell is selected by default (the cursor is on the first cell).
            selected: true,
            page: 1,
            row_limit: DEFAULT_RESULTS_ROW_LIMIT,
            ..ListState::default()
        }
    }

    /// The SQL the results pane actually executed, shown in the pane footer.
    /// Mirrors the original dbm's `paginated_executed_sql` status: a paginated
    /// run displays the SELECT rewritten with `LIMIT`/`OFFSET` (the statement
    /// that was really run), any other run displays the last SQL text. Empty
    /// when there is no result to describe.
    pub fn executed_sql_display(&self) -> String {
        let sql = self.last_sql.trim();
        if self.result.is_none() || sql.is_empty() {
            return String::new();
        }
        if self.paginated {
            let limit = self.row_limit as u64;
            let offset = (self.page.saturating_sub(1) as u64) * limit;
            dbm_driver_pg::paginated_select_sql(sql, limit, offset)
                .unwrap_or_else(|| sql.to_string())
        } else {
            sql.to_string()
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

    /// Total columns in the current result.
    pub fn column_count(&self) -> usize {
        self.result.as_ref().map(|r| r.columns.len()).unwrap_or(0)
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
            self.total_rows(),
            self.at_last_page,
        )
    }

    /// The total-row count stored on the current result, if known.
    pub fn total_rows(&self) -> Option<u64> {
        self.result.as_ref().and_then(|r| r.total_rows)
    }

    /// Whether the last query is a single count-able SELECT, so a COUNT total
    /// can be requested for it (the driver also auto-counts each paginated
    /// fetch; this covers result paths that did not carry a total).
    pub fn can_count_rows(&self) -> bool {
        self.paginated
            && self.result.is_some()
            && !self.last_sql.is_empty()
            && dbm_driver_pg::count_select_sql(&self.last_sql).is_some()
    }

    /// Whether the pagination toolbar should offer its `[c]count total rows`
    /// control: the query is count-able but its total is not known yet.
    pub fn show_count_button(&self) -> bool {
        self.can_count_rows() && self.total_rows().is_none()
    }

    /// The toolbar count flags (`counting`, `show_count_button`) shared by the
    /// renderer and the toolbar hit-testing so geometry always matches.
    pub fn toolbar_count_flags(&self) -> (bool, bool) {
        (self.counting, self.show_count_button())
    }

    /// Whether a previous SQL page exists (the cursor is on a page > 1).
    pub fn can_go_prev_page(&self) -> bool {
        self.paginated && self.page > 1
    }

    /// Whether the current result is editable (has an edit target).
    pub fn editable(&self) -> bool {
        self.edit_target.is_some()
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
        // Any navigation re-establishes the selection (leaving the deselect
        // state a cell is selected again).
        self.selected = true;
        let prev_row = self.row;
        let prev_col = self.col;
        if dr != 0 {
            self.row = if dr > 0 {
                (self.row + 1).min(row_count - 1)
            } else {
                self.row.saturating_sub(1)
            };
            // Scroll only when the cursor exits the visible viewport.
            // When viewport_rows is unknown (0), skip scrolling — the view
            // will auto-adjust v_scroll during render.
            let vr = self.viewport_rows.get();
            if vr > 0 {
                let vs = self.v_scroll.get();
                if self.row >= vs + vr {
                    self.v_scroll
                        .set(self.row.saturating_sub(vr.saturating_sub(1)));
                } else if self.row < vs {
                    self.v_scroll.set(self.row);
                }
                self.v_scroll
                    .set(self.v_scroll.get().min(row_count.saturating_sub(1)));
            }
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

    /// Recompute `search_matches` from the current query, scope and options,
    /// clamping the match index and selecting the active match. Used whenever
    /// the query, scope or ignore-case option changes.
    pub fn refresh_search_matches(&mut self) {
        self.search_matches = match self.result {
            Some(ref result) => super::search::find_matches(
                result,
                &self.search.query,
                self.search_scope_column,
                self.search.options,
            ),
            None => Vec::new(),
        };
        // Reading `result` above borrows; selection update happens separately.
        let count = self.search_matches.len();
        if count == 0 {
            self.search_match_index = 0;
            return;
        }
        if self.search_match_index >= count {
            self.search_match_index = 0;
        }
        self.apply_current_match();
    }

    /// Move the active match by `dr` (`+1` next, `-1` prev, wrapping) and
    /// select its cell. No-op when there are no matches.
    pub fn advance_search_match(&mut self, dr: i32) {
        let count = self.search_matches.len();
        if count == 0 {
            return;
        }
        self.search_match_index = if dr >= 0 {
            (self.search_match_index + 1) % count
        } else {
            (self.search_match_index + count - 1) % count
        };
        self.apply_current_match();
    }

    /// Select the cell of `search_matches[search_match_index]`.
    fn apply_current_match(&mut self) {
        let Some(m) = self.search_matches.get(self.search_match_index).copied() else {
            return;
        };
        // Navigating to a match selects its cell.
        self.selected = true;
        self.row = m.row;
        self.col = m.col;
    }

    /// The active match cell `(start, char_offset_into_value)` read-out, when
    /// there is a match (`count: idx/total` label uses this alongside `matches`).
    pub fn current_match_offset(&self) -> Option<(usize, usize)> {
        let m = self.search_matches.get(self.search_match_index)?;
        let value = self
            .result
            .as_ref()
            .and_then(|r| r.rows.get(m.row))
            .and_then(|r| r.get(m.col))?;
        Some((m.start, value.chars().count()))
    }

    /// The scope label for the current search (`col:{name}` or `all columns`).
    pub fn search_scope_label(&self) -> String {
        let name = self
            .search_scope_column
            .and_then(|c| self.result.as_ref().and_then(|r| r.columns.get(c)))
            .map(|meta| meta.name.as_str());
        super::search::search_scope_label(self.search_scope_column, name)
    }

    /// The read-out suffix for the search title: scope / count / offset.
    pub fn search_title_extra(&self) -> String {
        super::search::search_title_extra(
            &self.search,
            self.search_match_index,
            self.search_matches.len(),
            &self.search_scope_label(),
            self.current_match_offset(),
        )
    }

    /// Auto-adjust h_scroll to keep cursor column visible and anchored.
    /// Called during update when the column changes.
    ///
    /// Uses pixel-position comparison (not column indices) so that wide
    /// columns which are the sole visible column don't cause spurious
    /// left-scroll when moving to the previous column.
    pub fn auto_scroll_h(&mut self) {
        if self.col_widths.is_empty() {
            return;
        }
        let vp = self.viewport_width.get();
        if vp == 0 {
            return;
        }
        let viewport = vp as usize;
        let scroll = self.h_scroll.get();
        let view_right = scroll.saturating_add(viewport);

        let cur_col_left = crate::common::view::format::col_x_start(self.col, &self.col_widths);
        let cur_col_right = crate::common::view::format::col_x_end(self.col, &self.col_widths);

        let mut new_scroll = scroll;
        if cur_col_right <= scroll {
            new_scroll = cur_col_left;
        } else if cur_col_left >= view_right {
            new_scroll = cur_col_right.saturating_sub(viewport);
        }

        let table_w = crate::common::view::format::results_table_width(&self.col_widths) as usize;
        let max = table_w.saturating_sub(viewport);
        new_scroll = new_scroll.min(max);
        self.h_scroll.set(new_scroll);
    }

    /// Update the viewport dimensions (called from the rendering pass).
    pub fn set_viewport(&mut self, rows: usize, width: u16) {
        self.viewport_rows.set(rows.max(1));
        self.viewport_width.set(width);
        // Clamp v_scroll to the new viewport.
        let row_count = self.row_count();
        let max = row_count.saturating_sub(self.viewport_rows.get());
        self.v_scroll.set(self.v_scroll.get().min(max));
        // Clamp h_scroll to the new viewport.
        if !self.col_widths.is_empty() {
            let table_w =
                crate::common::view::format::results_table_width(&self.col_widths) as usize;
            let max_h = table_w.saturating_sub(width as usize);
            self.h_scroll.set(self.h_scroll.get().min(max_h));
        }
    }

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

    /// Roll back all edits, restoring snapshots into the result rows.
    pub fn rollback_edits(&mut self) {
        self.edit.rollback();
        if let Some(result) = self.result.as_mut() {
            result.rows = self.edit.snapshots.clone();
        }
    }

    /// Exit edit mode (clears the session).
    pub fn exit_edit(&mut self) {
        self.edit.exit_edit();
    }

    /// Apply an edited cell value into the edit session (and the live result).
    pub fn apply_cell_value(&mut self, row: usize, col: usize, value: String) {
        self.edit.apply_cell(row, col, value.clone());
        if let Some(result) = self.result.as_mut()
            && row < result.rows.len()
            && let Some(cell) = result.rows.get_mut(row).and_then(|r| r.get_mut(col))
        {
            *cell = value;
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
        let Some(values) = self
            .result
            .as_ref()
            .and_then(|r| r.rows.get(self.row))
            .cloned()
        else {
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
        super::super::edit_sql::build_commit_statements(target, &self.edit)
    }

    /// Commit-row count for the current edit session.
    pub fn commit_row_count(&self) -> usize {
        super::super::edit::commit_row_count(&self.edit)
    }

    /// Replace the current result and recompute column widths.
    pub fn set_result(&mut self, result: QueryResultData) {
        self.col_widths = init_results_layout(&result.columns, &result.rows);
        self.result = Some(result);
    }

    /// Where the next unsaved change sits, for cycling with the `m` key. Each
    /// changed row contributes one stop, in row order after the current row
    /// (wrapping to the first change when none follows): a pending insert or a
    /// deleted row lands on the row's first cell (column 0), an updated row on
    /// its first modified cell. Returns `None` when there is nothing to jump to
    /// — no active edit session, no pending changes, or the cursor already sits
    /// on the only stop.
    pub fn next_change(&self) -> Option<(usize, usize)> {
        let e = &self.edit;
        if !e.editing {
            return None;
        }
        let loaded = e.snapshots.len();
        let mut anchors: Vec<(usize, usize)> = Vec::new();
        for r in 0..loaded {
            if e.deleted.contains(&r) {
                anchors.push((r, 0));
            } else if let Some(first_col) = e
                .dirty_cells
                .keys()
                .filter(|(rr, _)| *rr == r)
                .map(|(_, c)| *c)
                .min()
            {
                anchors.push((r, first_col));
            }
        }
        for i in 0..e.new_rows.len() {
            anchors.push((loaded + i, 0));
        }
        if anchors.is_empty() {
            return None;
        }
        anchors.sort_unstable();
        let cursor = (self.row, self.col);
        let target = anchors
            .iter()
            .copied()
            .find(|a| *a > cursor)
            .unwrap_or(anchors[0]);
        (target != cursor).then_some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;
    use crate::features::sql_workspace::sql_tab::results::edit_sql;

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

    /// A ListState with an active edit session over three 3-column rows.
    fn editable() -> ListState {
        let mut s = ListState {
            result: Some(QueryResultData {
                columns: vec![col("a"), col("b"), col("c")],
                rows: vec![
                    vec!["1".into(), "2".into(), "3".into()],
                    vec!["4".into(), "5".into(), "6".into()],
                    vec!["7".into(), "8".into(), "9".into()],
                ],
                rows_affected: None,
                total_rows: Some(3),
            }),
            edit_target: Some(edit_sql::EditTarget {
                schema: "public".into(),
                table: "t".into(),
                primary_keys: vec!["a".into()],
                columns: vec!["a".into(), "b".into(), "c".into()],
            }),
            ..ListState::default()
        };
        s.enter_edit();
        s
    }

    #[test]
    fn next_change_jumps_to_update_first_dirty_cell() {
        let mut s = editable();
        s.apply_cell_value(0, 2, "changed".into()); // row 0 update, cell col2
        s.apply_cell_value(2, 0, "changed".into()); // row 2 update, cell col0
        // From the top: the first change is row 0's modified cell (col 2).
        s.row = 0;
        s.col = 0;
        assert_eq!(s.next_change(), Some((0, 2)));
        // From row 0's change: on to row 2's first modified cell.
        s.row = 0;
        s.col = 2;
        assert_eq!(s.next_change(), Some((2, 0)));
        // Past the last change wraps back to the first.
        s.row = 2;
        s.col = 0;
        assert_eq!(s.next_change(), Some((0, 2)));
    }

    #[test]
    fn next_change_lands_delete_and_insert_on_their_rows() {
        let mut s = editable();
        s.edit.mark_delete(1); // row 1 deleted
        s.edit_add_row(); // pending insert appended (row 3)
        // Row 1 is a delete, row 3 is an insert: both stop on column 0.
        s.row = 0;
        s.col = 0;
        assert_eq!(s.next_change(), Some((1, 0)));
        s.row = 1;
        assert_eq!(s.next_change(), Some((3, 0)));
        s.row = 3;
        assert_eq!(
            s.next_change(),
            Some((1, 0)),
            "wrap back to the first change"
        );
    }

    #[test]
    fn next_change_none_when_clean_or_at_the_only_stop() {
        // A clean session has no change stops.
        let mut clean = editable();
        assert_eq!(clean.next_change(), None);
        // With a single changed row, sitting on its stop means nothing to jump.
        clean.apply_cell_value(1, 1, "x".into());
        clean.row = 1;
        clean.col = 1;
        assert_eq!(clean.next_change(), None);
        // From another cell of the same row it still jumps to that stop.
        clean.col = 0;
        assert_eq!(clean.next_change(), Some((1, 1)));
    }

    #[test]
    fn move_selection_clamps_to_bounds() {
        let mut s = ListState {
            result: Some(sample()),
            ..ListState::default()
        };
        assert!(s.move_selection(1, 0));
        assert_eq!(s.row, 1);
        assert!(!s.move_selection(1, 0));
        assert_eq!(s.row, 1);
        assert!(s.move_selection(0, 1));
        assert_eq!(s.col, 1);
        assert!(!s.move_selection(0, 1));
        assert_eq!(s.col, 1);
    }

    #[test]
    fn selected_cell_returns_value() {
        let s = ListState {
            result: Some(sample()),
            col: 1,
            row: 1,
            ..ListState::default()
        };
        assert_eq!(s.selected_cell().as_deref(), Some("bob"));
        assert_eq!(s.selected_column_name(), Some("name"));
    }

    #[test]
    fn executed_sql_display_empty_without_result() {
        let s = ListState::default();
        assert_eq!(s.executed_sql_display(), "");
    }

    #[test]
    fn executed_sql_display_non_paginated_returns_trimmed_last_sql() {
        let s = ListState {
            result: Some(sample()),
            last_sql: "  SELECT id FROM t  ".into(),
            paginated: false,
            ..ListState::default()
        };
        assert_eq!(s.executed_sql_display(), "SELECT id FROM t");
    }

    #[test]
    fn executed_sql_display_paginated_wraps_with_limit_offset() {
        let s = ListState {
            result: Some(sample()),
            paginated: true,
            page: 2,
            row_limit: 100,
            last_sql: "SELECT id FROM t".into(),
            ..ListState::default()
        };
        assert_eq!(
            s.executed_sql_display(),
            "SELECT * FROM (SELECT id FROM t) AS dbm_page LIMIT 100 OFFSET 100"
        );
    }

    #[test]
    fn can_go_next_page_respects_pagination_and_total() {
        let mut s = ListState {
            result: Some(QueryResultData {
                rows: vec![vec!["1".into()]; 100],
                columns: vec![col("x")],
                rows_affected: None,
                total_rows: Some(300),
            }),
            paginated: true,
            row_limit: 100,
            page: 2,
            ..ListState::default()
        };
        assert!(s.can_go_next_page());
        s.page = 3;
        assert!(!s.can_go_next_page());
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
        let mut s = ListState {
            result: Some(sample()),
            edit_target: Some(edit_sql::EditTarget {
                schema: "public".into(),
                table: "users".into(),
                primary_keys: vec!["id".into()],
                columns: vec!["id".into(), "name".into()],
            }),
            ..ListState::default()
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
        let mut s = ListState {
            result: Some(sample()),
            edit_target: Some(edit_sql::EditTarget {
                schema: "public".into(),
                table: "users".into(),
                primary_keys: vec!["id".into()],
                columns: vec!["id".into(), "name".into()],
            }),
            ..ListState::default()
        };
        s.enter_edit();
        s.edit_add_row();
        assert_eq!(s.result.as_ref().unwrap().rows.len(), 3);
        assert_eq!(s.commit_row_count(), 1);
        s.row = 0;
        s.edit_dup_row();
        assert_eq!(s.result.as_ref().unwrap().rows.len(), 4);
        s.row = s.result.as_ref().unwrap().rows.len() - 1;
        s.edit_del_row();
        assert_eq!(s.result.as_ref().unwrap().rows.len(), 3);
    }

    #[test]
    fn build_commit_statements_from_edit_session() {
        let mut s = ListState {
            result: Some(sample()),
            edit_target: Some(edit_sql::EditTarget {
                schema: "public".into(),
                table: "users".into(),
                primary_keys: vec!["id".into()],
                columns: vec!["id".into(), "name".into()],
            }),
            ..ListState::default()
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
    fn rollback_edits_restores_snapshots() {
        let mut s = ListState {
            result: Some(sample()),
            edit_target: Some(edit_sql::EditTarget {
                schema: "public".into(),
                table: "users".into(),
                primary_keys: vec!["id".into()],
                columns: vec!["id".into(), "name".into()],
            }),
            ..ListState::default()
        };
        s.enter_edit();
        s.apply_cell_value(0, 1, "zoe".into());
        assert!(s.edit.is_dirty());
        s.rollback_edits();
        assert!(!s.edit.is_dirty());
        assert_eq!(s.result.as_ref().unwrap().rows[0][1], "alice");
    }
}
