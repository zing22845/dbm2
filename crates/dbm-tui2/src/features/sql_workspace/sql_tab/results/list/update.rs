//! Results list sub-module update.
//!
//! Pure by-value transition over the list state. Query execution runs as a
//! side-channel effect (`ResultsEffect::RunQuery` -> `Services::execute_sql`).

use crate::common::components::search::PaneSearchInput;

use super::super::effect::ResultsEffect;
use super::msg::ListMessage;
use super::state::ListState;

pub fn update(msg: ListMessage, mut state: ListState) -> (ListState, Vec<ResultsEffect>, bool) {
    let mut effects = Vec::new();
    let dirty = match msg {
        ListMessage::SetResult { result, paginated } => {
            state.set_result(result);
            state.query_error = None;
            state.paginated = paginated;
            state.row = 0;
            state.col = 0;
            // A fresh result is deselected (matching the original dbm, which
            // clears the cell selection after every query run).
            state.selected = false;
            state.h_scroll.set(0);
            state.search.reset();
            state.search_matches.clear();
            state.search_match_index = 0;
            state.search_scope_column = None;
            state.edit_target = None;
            state.edit_blocked_reason = None;
            let result_columns: Vec<String> = state
                .result
                .as_ref()
                .map(|r| r.columns.iter().map(|c| c.name.clone()).collect())
                .unwrap_or_default();
            if !result_columns.is_empty() && !state.last_sql.is_empty() {
                effects.push(ResultsEffect::CheckEditability {
                    instance: state.last_instance.clone(),
                    connection: state.last_connection.clone(),
                    database: state.last_database.clone(),
                    schema: state.last_schema.clone(),
                    sql: state.last_sql.clone(),
                    result_columns,
                });
            }
            true
        }
        ListMessage::EditabilityReady { target, blocked } => {
            state.edit_target = target;
            state.edit_blocked_reason = blocked;
            if state.edit_target.is_none() {
                state.exit_edit();
            }
            true
        }
        ListMessage::ClearResult => {
            let changed = state.result.is_some();
            state.result = None;
            state.col_widths.clear();
            state.query_error = None;
            state.row = 0;
            state.col = 0;
            state.search.reset();
            state.search_matches.clear();
            state.search_match_index = 0;
            state.search_scope_column = None;
            state.edit_target = None;
            state.edit_blocked_reason = None;
            changed
        }
        ListMessage::QueryError { message } => {
            let changed =
                state.result.is_some() || state.query_error.as_deref() != Some(message.as_str());
            state.result = None;
            state.col_widths.clear();
            state.query_error = Some(message);
            state.row = 0;
            state.col = 0;
            state.search.reset();
            state.search_matches.clear();
            state.search_match_index = 0;
            state.search_scope_column = None;
            state.edit_target = None;
            state.edit_blocked_reason = None;
            changed
        }
        ListMessage::MoveSelection { dr, dc } => {
            state.scroll_locked.set(false);
            let changed = state.move_selection(dr, dc);
            if changed && dc != 0 {
                state.auto_scroll_h();
            }
            changed
        }
        ListMessage::SetSelection { row, col } => {
            let result = state.result.as_ref();
            let max_row = result.map(|r| r.rows.len().saturating_sub(1)).unwrap_or(0);
            let max_col = result
                .map(|r| r.columns.len().saturating_sub(1))
                .unwrap_or(0);
            let row = row.min(max_row);
            let col = col.min(max_col);
            let changed = state.row != row || state.col != col;
            state.selected = true;
            state.row = row;
            state.col = col;
            if changed {
                state.auto_scroll_h();
            }
            changed
        }
        ListMessage::BeginSearch => {
            // Re-enter search editing on the existing filter: unlike the
            // initial open, a repeated `/` must preserve the previous keyword
            // (matching the original dbm) so it can be edited/refined.
            state.search.start();
            state.search_match_index = 0;
            state.search_matches.clear();
            // Scope the search to the selected column; `None` (all columns, a
            // full-text match) when the grid has been deselected or has no
            // result yet — mirroring the original dbm's `start_search`.
            state.search_scope_column = if state.selected && state.col < state.column_count() {
                Some(state.col)
            } else {
                None
            };
            state.refresh_search_matches();
            true
        }
        ListMessage::SearchKey(key) => handle_search_key(&mut state, key),
        ListMessage::SearchNavigate { forward } => {
            state.advance_search_match(if forward { 1 } else { -1 });
            true
        }
        ListMessage::ResetSelection => {
            // True deselect (matching the original dbm's `select_cell(None)`):
            // no cell cursor is shown, and a search started from here matches
            // all columns.
            let changed = state.row != 0
                || state.col != 0
                || state.h_scroll.get() != 0
                || state.search.active
                || state.selected;
            state.selected = false;
            state.row = 0;
            state.col = 0;
            state.h_scroll.set(0);
            state.search.reset();
            changed
        }
        ListMessage::RunQuery {
            instance,
            connection,
            database,
            schema,
            sql,
            paginated,
            page,
            row_limit,
        } => {
            state.query_error = None;
            state.last_sql = sql.clone();
            state.last_instance = instance.clone();
            state.last_connection = connection.clone();
            state.last_database = database.clone();
            state.last_schema = schema.clone();
            effects.push(ResultsEffect::RunQuery {
                instance,
                connection,
                database,
                schema,
                sql,
                paginated,
                page,
                row_limit,
            });
            true
        }
        ListMessage::EnterEdit => {
            state.enter_edit();
            true
        }
        ListMessage::ExitEdit => {
            state.exit_edit();
            true
        }
        ListMessage::Rollback => {
            state.rollback_edits();
            true
        }
        ListMessage::AddRow => {
            state.edit_add_row();
            true
        }
        ListMessage::DupRow => {
            state.edit_dup_row();
            true
        }
        ListMessage::DelRow => {
            state.edit_del_row();
            true
        }
        ListMessage::SetRowLimit { limit } => {
            state.row_limit = limit.max(1);
            state.page = 1;
            rerun_query(&state, &mut effects);
            true
        }
        ListMessage::SetPage { page } => {
            let max = super::super::pagination::max_page(
                state.result.as_ref().and_then(|r| r.total_rows),
                state.row_limit,
            );
            state.page = page.max(1);
            if let Some(max) = max {
                state.page = state.page.min(max.max(1));
            }
            rerun_query(&state, &mut effects);
            true
        }
        ListMessage::Commit => {
            if let Ok(statements) = state.build_commit_statements() {
                effects.push(ResultsEffect::Commit {
                    instance: state.last_instance.clone(),
                    connection: state.last_connection.clone(),
                    database: state.last_database.clone(),
                    schema: state.last_schema.clone(),
                    statements,
                });
            }
            false
        }
        ListMessage::SyncViewport { rows, width } => {
            state.set_viewport(rows, width);
            false
        }
        ListMessage::SetVScroll { position } => {
            let row_count = state.row_count();
            let vr = state.viewport_rows.get().max(1);
            let max = row_count.saturating_sub(vr);
            let before = state.v_scroll.get();
            state.scroll_locked.set(true);
            state.v_scroll.set(position.min(max));
            state.v_scroll.get() != before
        }
        ListMessage::SetHScroll { position } => {
            let before = state.h_scroll.get();
            let max = max_h_scroll(&state);
            state.scroll_locked.set(true);
            state.h_scroll.set(position.min(max));
            state.h_scroll.get() != before
        }
        ListMessage::ScrollHScroll { delta } => {
            let before = state.h_scroll.get();
            let max = max_h_scroll(&state);
            state.scroll_locked.set(true);
            let next = if delta >= 0 {
                before.saturating_add(delta as usize).min(max)
            } else {
                before.saturating_sub(delta.unsigned_abs() as usize)
            };
            state.h_scroll.set(next);
            next != before
        }
        ListMessage::AdjustColWidth { delta } => {
            use crate::common::view::format::{MAX_RESULTS_COL_WIDTH, MIN_RESULTS_COL_WIDTH};
            let Some(width) = state.col_widths.get_mut(state.col) else {
                return (state, effects, false);
            };
            let next = (*width as i16 + delta)
                .clamp(MIN_RESULTS_COL_WIDTH as i16, MAX_RESULTS_COL_WIDTH as i16)
                as u16;
            let changed = next != *width;
            *width = next;
            changed
        }
        ListMessage::AdjustColWidthTo { col, width } => {
            use crate::common::view::format::{MAX_RESULTS_COL_WIDTH, MIN_RESULTS_COL_WIDTH};
            let Some(cw) = state.col_widths.get_mut(col) else {
                return (state, effects, false);
            };
            let next = width.clamp(MIN_RESULTS_COL_WIDTH, MAX_RESULTS_COL_WIDTH);
            let changed = next != *cw;
            *cw = next;
            changed
        }
    };
    (state, effects, dirty)
}

/// Maximum horizontal scroll offset for the current column widths and viewport.
///
/// Shared by the absolute (`SetHScroll`) and relative (`ScrollHScroll`) handlers
/// so both clamp against the same bound — the wheel cannot scroll past the
/// point where the last column's right edge meets the viewport's right edge.
fn max_h_scroll(state: &ListState) -> usize {
    let table_w = crate::common::view::format::results_table_width(&state.col_widths) as usize;
    let vp = state.viewport_width.get() as usize;
    table_w.saturating_sub(vp)
}

/// Re-run the last query with the current page/row-limit.
fn rerun_query(state: &ListState, effects: &mut Vec<ResultsEffect>) {
    if state.last_sql.is_empty()
        || state.last_instance.is_empty()
        || state.last_connection.is_empty()
    {
        return;
    }
    effects.push(ResultsEffect::RunQuery {
        instance: state.last_instance.clone(),
        connection: state.last_connection.clone(),
        database: state.last_database.clone(),
        schema: state.last_schema.clone(),
        sql: state.last_sql.clone(),
        paginated: state.paginated,
        page: state.page,
        row_limit: state.row_limit,
    });
}

fn handle_search_key(state: &mut ListState, key: crossterm::event::KeyEvent) -> bool {
    let caps_lock = false;
    let action = match key.code {
        crossterm::event::KeyCode::Esc => {
            state.search.reset();
            state.search_match_index = 0;
            state.search_matches.clear();
            state.search_scope_column = None;
            PaneSearchInput::Cancelled
        }
        crossterm::event::KeyCode::Enter => {
            state.search.end();
            PaneSearchInput::Applied
        }
        _ => state.search.handle_key(&key, caps_lock),
    };

    match action {
        // Ctrl+p / Ctrl+n move between matches, wrapping.
        PaneSearchInput::Navigate { forward } => {
            state.advance_search_match(if forward { 1 } else { -1 });
        }
        // Query or case-option change: re-run the match over the grid and
        // select the first match.
        PaneSearchInput::QueryChanged | PaneSearchInput::OptionsChanged => {
            state.refresh_search_matches();
        }
        _ => {}
    }

    // A key the search input ignored (e.g. holding a direction key while
    // typing) changes nothing, so do not mark the state dirty. This mirrors
    // the original dbm, which returns `Unchanged` for ignored search keys and
    // avoids re-rendering the whole grid on every key-press repeat.
    action != PaneSearchInput::Ignored
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;

    fn sample_result() -> super::super::super::state::QueryResultData {
        super::super::super::state::QueryResultData {
            columns: vec![ColumnInfo {
                name: "id".into(),
                type_name: "int4".into(),
                type_display: "int4".into(),
                comment: None,
            }],
            rows: vec![vec!["1".into()]],
            rows_affected: None,
            total_rows: Some(1),
        }
    }

    #[test]
    fn query_error_clears_result_and_stores_message() {
        let (mut state, _e, dirty) = update(
            ListMessage::SetResult {
                result: sample_result(),
                paginated: false,
            },
            ListState::default(),
        );
        assert!(state.result.is_some());
        assert!(dirty);

        let (s, _e, dirty) = update(
            ListMessage::QueryError {
                message: "relation \"nope\" does not exist".into(),
            },
            state,
        );
        state = s;
        assert!(state.result.is_none());
        assert_eq!(
            state.query_error.as_deref(),
            Some("relation \"nope\" does not exist")
        );
        assert!(dirty);
    }

    #[test]
    fn set_result_deselects_cell() {
        // The original dbm clears the cell selection after every query run, so
        // a fresh result must be deselected even when the state was selected.
        let (state, _e, _dirty) = update(
            ListMessage::SetResult {
                result: sample_result(),
                paginated: false,
            },
            ListState::new(), // `new()` starts selected; SetResult must reset it
        );
        assert!(state.result.is_some());
        assert!(!state.selected, "a fresh result must be deselected");
    }

    #[test]
    fn scroll_h_scroll_moves_relative_and_clamps() {
        // A table wider than its viewport is what makes horizontal wheeling
        // meaningful: the wheel carries a relative delta and the feature layer
        // owns clamping, so the loop layer never needs to know the bound.
        let mut state = ListState::default();
        state.set_result(sample_result());
        state.col_widths = vec![60];
        state.viewport_width.set(20);
        state.h_scroll.set(0);

        let (s, _e, dirty) = update(ListMessage::ScrollHScroll { delta: 5 }, state);
        state = s;
        assert!(dirty);
        assert_eq!(state.h_scroll.get(), 5);

        // Scrolling back by the same amount returns to the origin.
        let (s, _e, dirty) = update(ListMessage::ScrollHScroll { delta: -5 }, state);
        state = s;
        assert!(dirty);
        assert_eq!(state.h_scroll.get(), 0);

        // Already at the left edge: another left tick changes nothing, so it
        // must not mark the pane dirty (no repaint for a no-op gesture).
        let (s, _e, dirty) = update(ListMessage::ScrollHScroll { delta: -5 }, state);
        state = s;
        assert!(!dirty);
        assert_eq!(state.h_scroll.get(), 0);

        // Overshooting right clamps at the content edge instead of scrolling
        // into blank space, and takes the anchor lock like a scrollbar drag.
        let (s, _e, _dirty) = update(ListMessage::ScrollHScroll { delta: 10_000 }, state);
        state = s;
        assert_eq!(state.h_scroll.get(), max_h_scroll(&state));
        assert!(state.scroll_locked.get());
    }

    #[test]
    fn ignored_search_keys_do_not_mark_dirty() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut state = ListState::default();
        state.search.start();

        // Holding a direction key while the search input is live is ignored by
        // the search component, so it must not force a full grid re-render.
        let (s, _e, dirty) = update(
            ListMessage::SearchKey(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            state,
        );
        state = s;
        assert!(
            !dirty,
            "an ignored search key must not mark the state dirty"
        );

        let (s, _e, dirty) = update(
            ListMessage::SearchKey(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
            state,
        );
        state = s;
        assert!(dirty, "a query character must mark the state dirty");
        assert_eq!(state.search.query, "a");
    }

    #[test]
    fn deselect_makes_search_full_text_scope() {
        let base = ListState {
            result: Some(sample_result()),
            selected: true,
            row: 0,
            col: 0,
            ..ListState::default()
        };
        // With a selected cell, search scopes to the selected column.
        let (mut state, _e, _d) = update(ListMessage::BeginSearch, base.clone());
        assert_eq!(state.search_scope_column, Some(0));
        state.search.reset();

        // Esc deselection: no cell cursor; a fresh search matches all columns.
        let (state, _e, _d) = update(ListMessage::ResetSelection, state);
        assert!(!state.selected);
        let (state, _e, _d) = update(ListMessage::BeginSearch, state);
        assert!(state.search_scope_column.is_none());
    }

    #[test]
    fn set_result_clears_previous_query_error() {
        let (mut state, _e, _d) = update(
            ListMessage::QueryError {
                message: "boom".into(),
            },
            ListState::default(),
        );
        assert!(state.query_error.is_some());

        let (s, _e, _d) = update(
            ListMessage::SetResult {
                result: sample_result(),
                paginated: false,
            },
            state,
        );
        state = s;
        assert!(state.result.is_some());
        assert!(state.query_error.is_none());
    }

    #[test]
    fn clear_result_clears_query_error_too() {
        let (state, _e, _d) = update(
            ListMessage::QueryError {
                message: "boom".into(),
            },
            ListState::default(),
        );
        assert!(state.query_error.is_some());

        let (s, _e, _d) = update(ListMessage::ClearResult, state);
        assert!(s.query_error.is_none());
    }

    #[test]
    fn adjust_col_width_shifts_selected_column_and_clamps() {
        // `,` / `.` step the selected column's width by the configured delta.
        let (state, _e, _d) = update(
            ListMessage::SetResult {
                result: sample_result(),
                paginated: false,
            },
            ListState::default(),
        );
        let start = state.col_widths[0];

        let (s, _e, dirty) = update(ListMessage::AdjustColWidth { delta: 2 }, state);
        assert!(dirty);
        let state = s;
        assert_eq!(state.col_widths[0], start + 2, "widening adds the step");

        // A negative delta narrows the column.
        let (s, _e, _d) = update(ListMessage::AdjustColWidth { delta: -4 }, state);
        assert_eq!(s.col_widths[0], start - 2, "narrowing subtracts the step");

        // The width is clamped to the permitted range (both directions).
        let (s, _e, _d) = update(ListMessage::AdjustColWidth { delta: -100 }, s);
        assert_eq!(
            s.col_widths[0],
            crate::common::view::format::MIN_RESULTS_COL_WIDTH
        );

        let (s, _e, _d) = update(ListMessage::AdjustColWidth { delta: 100 }, s);
        assert_eq!(
            s.col_widths[0],
            crate::common::view::format::MAX_RESULTS_COL_WIDTH
        );
    }

    #[test]
    fn adjust_col_width_to_sets_drag_target_and_clamps() {
        // A mouse drag on a column's header splitter sets an absolute width.
        let (state, _e, _d) = update(
            ListMessage::SetResult {
                result: sample_result(),
                paginated: false,
            },
            ListState::default(),
        );
        let (s, _e, dirty) = update(ListMessage::AdjustColWidthTo { col: 0, width: 30 }, state);
        assert!(dirty);
        assert_eq!(s.col_widths[0], 30);

        // Out-of-range targets are clamped.
        let (s, _e, _d) = update(ListMessage::AdjustColWidthTo { col: 0, width: 1 }, s);
        assert_eq!(
            s.col_widths[0],
            crate::common::view::format::MIN_RESULTS_COL_WIDTH
        );
        let (s, _e, _d) = update(
            ListMessage::AdjustColWidthTo {
                col: 0,
                width: 5000,
            },
            s,
        );
        assert_eq!(
            s.col_widths[0],
            crate::common::view::format::MAX_RESULTS_COL_WIDTH
        );

        // An unknown column is a safe no-op.
        let (_s, _e, dirty) = update(ListMessage::AdjustColWidthTo { col: 99, width: 30 }, s);
        assert!(!dirty);
    }
}
