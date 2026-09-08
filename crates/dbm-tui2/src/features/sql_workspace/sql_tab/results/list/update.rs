//! Results list sub-module update.
//!
//! Pure by-value transition over the list state. Query execution runs as a
//! side-channel effect (`ResultsEffect::RunQuery` -> `Services::execute_sql`).

use crate::common::components::search::PaneSearchInput;

use super::super::effect::ResultsEffect;
use super::super::pagination::{ResultsPageAction, max_page, page_count, resolve_page_chord};
use super::msg::ListMessage;
use super::state::ListState;

pub fn update(msg: ListMessage, mut state: ListState) -> (ListState, Vec<ResultsEffect>, bool) {
    let mut effects = Vec::new();
    // An armed `dd` chord must be broken by anything in between except another
    // `d` and the per-frame viewport sync (which is not a user action): typing
    // or moving between the two presses must not still trigger the delete.
    if !matches!(&msg, ListMessage::DelChord) && !matches!(&msg, ListMessage::SyncViewport { .. }) {
        state.del_chord_at = None;
    }
    let dirty = match msg {
        ListMessage::SetResult { result, paginated } => {
            let mut result = result;
            let row_count = result.rows.len();
            // Lazy total, like the original dbm: paginated page fetches carry
            // no grand total. When a non-empty page comes back short of the
            // page size it is the last page, so the total is the rows seen so
            // far (`offset + row_count`); a first page short page therefore
            // reports its own row count. A previously counted total is
            // re-applied so the "page x/N" toolbar survives further pages.
            let inferred_total = if paginated && row_count > 0 && row_count < state.row_limit {
                let offset = (state.page.saturating_sub(1) as u64) * state.row_limit as u64;
                Some(offset + row_count as u64)
            } else {
                None
            };
            let effective_total = if paginated {
                result.total_rows.or(state.total_cache).or(inferred_total)
            } else {
                None
            };
            if paginated {
                state.total_cache = effective_total;
            } else {
                state.total_cache = None;
            }
            result.total_rows = effective_total;
            // A page-nav anchor decided where the cursor of the incoming page
            // lands (Prev/Last -> bottom). Consumed once per landed result.
            let anchor_bottom = state.page_anchor_bottom;
            state.page_anchor_bottom = false;
            // A landed result supersedes any in-flight count / queued nav.
            state.counting = false;
            state.pending_page_after_count = None;
            // Derive whether this page is the last reachable one so page-nav
            // and the `at_last_page` guard stay consistent across runs.
            state.at_last_page = if paginated {
                match effective_total {
                    Some(total) => state.page >= page_count(total, state.row_limit).max(1),
                    None => row_count < state.row_limit || row_count == 0,
                }
            } else {
                true
            };
            state.set_result(result);
            state.query_error = None;
            state.paginated = paginated;
            if anchor_bottom && row_count > 0 {
                // Landing on a page reached by moving backwards: park the
                // cursor on its last row and scroll the viewport to the bottom
                // (mirroring the original dbm's `results_scroll_to_page_bottom`
                // after a Prev/Last nav).
                state.row = row_count - 1;
                state.col = 0;
                state.selected = true;
                let vr = state.viewport_rows.get().max(1);
                state.v_scroll.set(row_count.saturating_sub(vr));
            } else {
                state.row = 0;
                state.col = 0;
                state.v_scroll.set(0);
                // A fresh result is deselected (matching the original dbm, which
                // clears the cell selection after every query run).
                state.selected = false;
            }
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
            state.at_last_page = true;
            state.page_anchor_bottom = false;
            state.page_chord = None;
            state.counting = false;
            state.pending_page_after_count = None;
            state.total_cache = None;
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
            state.at_last_page = true;
            state.page_anchor_bottom = false;
            state.page_chord = None;
            state.counting = false;
            state.pending_page_after_count = None;
            state.total_cache = None;
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
        ListMessage::CopyColumnName => {
            // Copy the selected column's name; with no cell selected, copy all
            // column names joined by ", " (matching the original dbm).
            state.page_chord = None;
            let Some(result) = state.result.as_ref() else {
                return (state, effects, false);
            };
            let text = if state.selected && state.col < result.columns.len() {
                result.columns[state.col].name.clone()
            } else {
                result
                    .columns
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            if text.is_empty() {
                false
            } else {
                effects.push(ResultsEffect::CopyColumnName { text });
                true
            }
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
            // A fresh run restarts the cursor anchoring at the top; only a
            // page-nav (`PageNav`) sets the bottom anchor for its landing page.
            state.page_anchor_bottom = false;
            state.page_chord = None;
            state.counting = false;
            state.pending_page_after_count = None;
            // A new (different) query must not inherit a previously counted
            // grand total; re-running the identical statement may keep it.
            if state.last_sql != sql {
                state.total_cache = None;
            }
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
            // Entering an edit session re-snapshots the rows and wipes any
            // pending dirty cells/deleted/new rows. Guard against re-entry so
            // an already-active session (repeat `i`, or clicking the Edit
            // toolbar button) can never silently drop edits.
            if state.edit.editing {
                false
            } else {
                state.enter_edit();
                true
            }
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
            state.del_chord_at = None;
            true
        }
        ListMessage::DelChord => {
            // `dd`: a second `d` within 500 ms deletes the selected row; a
            // lone `d` only arms the chord (mirrors the original dbm's
            // `edit_del_row` guard in `handle_edit_table_key`).
            const DEL_CHORD_WINDOW_MS: u128 = 500;
            let now = std::time::Instant::now();
            let second = state
                .del_chord_at
                .is_some_and(|at| now.duration_since(at).as_millis() <= DEL_CHORD_WINDOW_MS);
            state.del_chord_at = Some(now);
            if second {
                state.edit_del_row();
                true
            } else {
                false
            }
        }
        ListMessage::DelChordCancel => {
            // An intervening key (bound or not) cleared the arm at the top of
            // `update`; nothing else to do.
            state.del_chord_at = None;
            false
        }
        ListMessage::SetRowLimit { limit } => {
            if state.edit.editing && state.edit.is_dirty() {
                return (state, effects, false);
            }
            state.page_chord = None;
            state.row_limit = limit.max(1);
            state.page = 1;
            // A new page size re-runs from page 1, landing at the top.
            state.page_anchor_bottom = false;
            rerun_query(&state, &mut effects);
            true
        }
        ListMessage::SetPage { page } => {
            if state.edit.editing && state.edit.is_dirty() {
                return (state, effects, false);
            }
            state.page_chord = None;
            let max = max_page(
                state.result.as_ref().and_then(|r| r.total_rows),
                state.row_limit,
            );
            state.page = page.max(1);
            if let Some(max) = max {
                state.page = state.page.min(max.max(1));
            }
            // A direct page jump parks the cursor at the top of the target
            // page (the landing page's row anchoring is set only by PageNav).
            state.page_anchor_bottom = false;
            rerun_query(&state, &mut effects);
            true
        }
        ListMessage::PageNav { action } => {
            // Toolbar / boundary navigation is not part of the `<`/`>` chord:
            // using it disarms any pending double-press.
            state.page_chord = None;
            apply_page_action(&mut state, &mut effects, action)
        }
        ListMessage::PageChord { forward } => {
            // `<`/`>` single-step nav that arms the double-press chord; a second
            // press of the same key within the window upgrades to first/last.
            let (pending, action) =
                resolve_page_chord(state.page_chord.take(), std::time::Instant::now(), forward);
            state.page_chord = pending;
            match action {
                Some(action) => apply_page_action(&mut state, &mut effects, action),
                None => false,
            }
        }
        ListMessage::CountRows => {
            // `c` / `[c]count total rows`: request COUNT over the last query
            // while its total is still unknown.
            if state.counting || !state.can_count_rows() {
                false
            } else {
                state.counting = true;
                state.pending_page_after_count = None;
                push_count(&state, &mut effects);
                true
            }
        }
        ListMessage::CountReady { sql, total } => {
            if state.last_sql != sql {
                // A stale count (the query changed while it was in flight).
                false
            } else {
                let mut changed = state.counting;
                state.counting = false;
                if let Some(total) = total {
                    state.total_cache = Some(total);
                    if let Some(result) = state.result.as_mut() {
                        result.total_rows = Some(total);
                    }
                    changed = true;
                }
                // A page action queued behind the count (last page) resumes.
                if let Some(action) = state.pending_page_after_count.take() {
                    let nav = state.total_rows().is_some()
                        && apply_page_action(&mut state, &mut effects, action);
                    changed = changed || nav;
                }
                // Derive the page-end flag from the (possibly moved) current
                // page and the freshly landed total.
                if let Some(total) = state.total_rows() {
                    state.at_last_page = state.page >= page_count(total, state.row_limit).max(1);
                }
                changed
            }
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

/// Push a COUNT-total effect for the last query (requires the connection
/// context captured by the preceding run).
fn push_count(state: &ListState, effects: &mut Vec<ResultsEffect>) {
    if state.last_sql.is_empty()
        || state.last_instance.is_empty()
        || state.last_connection.is_empty()
    {
        return;
    }
    effects.push(ResultsEffect::CountRows {
        instance: state.last_instance.clone(),
        connection: state.last_connection.clone(),
        database: state.last_database.clone(),
        schema: state.last_schema.clone(),
        sql: state.last_sql.clone(),
    });
}

/// Apply a page action (First / Prev / Next / Last / Set): resolve the target
/// page from the live total, guard the page edges, and re-run the query.
/// Shared by the toolbar nav (`PageNav`) and the `<`/`>` chord (`PageChord`).
/// Returns whether a re-run was scheduled. When "last page" is requested with
/// no total known yet but the query is count-able, a COUNT is dispatched first
/// and the jump resumes when the total lands (mirroring the original dbm).
fn apply_page_action(
    state: &mut ListState,
    effects: &mut Vec<ResultsEffect>,
    action: ResultsPageAction,
) -> bool {
    if state.edit.editing && state.edit.is_dirty() {
        return false;
    }
    let (next, anchor_bottom) = match action {
        ResultsPageAction::First => (1usize, false),
        ResultsPageAction::Prev => (state.page.saturating_sub(1).max(1), true),
        ResultsPageAction::Next => {
            if !state.can_go_next_page() {
                (state.page, false)
            } else {
                (state.page + 1, false)
            }
        }
        ResultsPageAction::Last => match state.total_rows() {
            Some(total) => (page_count(total, state.row_limit).max(1), true),
            None if state.can_count_rows() && !state.counting => {
                // Unknown total: COUNT first, then resume the last-page jump
                // from `CountReady`.
                state.counting = true;
                state.pending_page_after_count = Some(ResultsPageAction::Last);
                push_count(state, effects);
                return true;
            }
            None => (state.page, false),
        },
        ResultsPageAction::Set(page) => (page, false),
    };
    if next == state.page {
        return false;
    }
    state.page = next;
    state.page_anchor_bottom = anchor_bottom;
    rerun_query(state, effects);
    true
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

    fn paginated_state(page: usize, total: Option<u64>, rows: usize) -> ListState {
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;
        ListState {
            result: Some(super::super::super::state::QueryResultData {
                columns: vec![ColumnInfo {
                    name: "x".into(),
                    type_name: "int4".into(),
                    type_display: "int4".into(),
                    comment: None,
                }],
                rows: vec![vec!["1".into()]; rows],
                rows_affected: None,
                total_rows: total,
            }),
            paginated: true,
            page,
            row_limit: 100,
            selected: true,
            // `rerun_query` needs the last-run connection context to re-issue.
            last_sql: "SELECT * FROM t".into(),
            last_instance: "inst".into(),
            last_connection: "conn".into(),
            last_schema: "public".into(),
            ..ListState::default()
        }
    }

    fn rerun_page(effects: &[ResultsEffect]) -> Option<usize> {
        effects.iter().find_map(|e| match e {
            ResultsEffect::RunQuery { page, .. } => Some(*page),
            _ => None,
        })
    }

    #[test]
    fn page_nav_next_increments_page_and_reruns() {
        let (s, effects, dirty) = update(
            ListMessage::PageNav {
                action: super::super::super::pagination::ResultsPageAction::Next,
            },
            paginated_state(1, Some(300), 100),
        );
        assert!(dirty);
        assert_eq!(rerun_page(&effects), Some(2), "Next re-runs page 2");
        assert_eq!(s.page, 2);
        assert!(
            !s.page_anchor_bottom,
            "moving forward anchors the landing page at the top"
        );
    }

    #[test]
    fn page_nav_next_at_last_page_is_a_noop() {
        let (s, effects, dirty) = update(
            ListMessage::PageNav {
                action: super::super::super::pagination::ResultsPageAction::Next,
            },
            paginated_state(3, Some(300), 100),
        );
        assert!(!dirty);
        assert_eq!(s.page, 3);
        assert!(effects.is_empty(), "no re-run beyond the last page");
    }

    #[test]
    fn page_nav_prev_decrements_and_anchors_bottom() {
        let (s, effects, dirty) = update(
            ListMessage::PageNav {
                action: super::super::super::pagination::ResultsPageAction::Prev,
            },
            paginated_state(2, Some(300), 100),
        );
        assert!(dirty);
        assert_eq!(rerun_page(&effects), Some(1));
        assert_eq!(s.page, 1);
        assert!(
            s.page_anchor_bottom,
            "moving backwards anchors the landing page at the bottom"
        );
    }

    #[test]
    fn page_nav_last_uses_total_to_compute_final_page() {
        let (s, effects, dirty) = update(
            ListMessage::PageNav {
                action: super::super::super::pagination::ResultsPageAction::Last,
            },
            paginated_state(1, Some(300), 100),
        );
        assert!(dirty);
        assert_eq!(rerun_page(&effects), Some(3));
        assert_eq!(s.page, 3);
        assert!(
            s.page_anchor_bottom,
            "Last lands on the final page's bottom"
        );

        // Without a total the last page is unknowable: a COUNT is dispatched
        // first and the jump resumes when the total lands.
        let (s, effects, dirty) = update(
            ListMessage::PageNav {
                action: super::super::super::pagination::ResultsPageAction::Last,
            },
            paginated_state(1, None, 100),
        );
        assert!(dirty, "queuing the count must redraw the counting state");
        assert_eq!(s.page, 1, "the page does not move until the total lands");
        assert!(s.counting);
        assert_eq!(s.pending_page_after_count, Some(ResultsPageAction::Last));
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, ResultsEffect::CountRows { .. })),
            "an unknown-total last-page jump must dispatch a COUNT"
        );
    }

    #[test]
    fn page_chord_single_press_pages_and_arms_chord() {
        // A single `>` pages forward like PageNav but keeps the armed chord
        // for a potential second press.
        let (s, effects, dirty) = update(
            ListMessage::PageChord { forward: true },
            paginated_state(1, Some(300), 100),
        );
        assert!(dirty);
        assert_eq!(rerun_page(&effects), Some(2));
        assert_eq!(s.page, 2);
        assert!(
            s.page_chord.is_some(),
            "a first `>` must arm the double-press chord"
        );
    }

    #[test]
    fn page_nav_disarms_armed_chord() {
        // A toolbar / boundary nav is not a chord key: using one clears any
        // armed `<`/`>` chord so the next `>` starts a fresh single page.
        let mut state = paginated_state(1, Some(300), 100);
        state.page_chord = Some((true, std::time::Instant::now()));
        let (s, _e, dirty) = update(
            ListMessage::PageNav {
                action: super::super::super::pagination::ResultsPageAction::Next,
            },
            state,
        );
        assert!(dirty);
        assert!(s.page_chord.is_none(), "PageNav must clear the chord");
        assert_eq!(s.page, 2);
    }

    #[test]
    fn count_rows_dispatches_when_allowed_and_marks_counting() {
        let (s, effects, dirty) = update(ListMessage::CountRows, paginated_state(1, None, 100));
        assert!(dirty);
        assert!(s.counting, "a dispatched count arms the counting flag");
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, ResultsEffect::CountRows { .. })),
            "CountRows must push a COUNT effect"
        );

        // A second request while one is in flight is a no-op.
        let (s, effects, dirty) = update(ListMessage::CountRows, s);
        assert!(!dirty);
        assert!(s.counting);
        assert!(effects.is_empty());
    }

    #[test]
    fn count_ready_applies_total_and_resumes_queued_last_page() {
        // An unknown-total "last page" jump queued a count and a pending nav.
        let mut state = paginated_state(1, None, 100);
        state.counting = true;
        state.pending_page_after_count =
            Some(super::super::super::pagination::ResultsPageAction::Last);
        let (s, effects, dirty) = update(
            ListMessage::CountReady {
                sql: "SELECT * FROM t".into(),
                total: Some(300),
            },
            state,
        );
        assert!(dirty);
        assert!(!s.counting);
        assert_eq!(s.total_rows(), Some(300));
        assert_eq!(
            rerun_page(&effects),
            Some(3),
            "the queued last-page jump runs once the total lands"
        );
        assert_eq!(s.page, 3);
        assert!(s.at_last_page, "page 3 of 3 is the last page");
        assert!(
            s.pending_page_after_count.is_none(),
            "the queued nav is consumed"
        );
    }

    #[test]
    fn stale_count_ready_is_ignored() {
        let (s, effects, dirty) = update(
            ListMessage::CountReady {
                sql: "SELECT * FROM other".into(),
                total: Some(9),
            },
            paginated_state(1, Some(300), 100),
        );
        assert!(!dirty);
        assert!(effects.is_empty());
        assert_eq!(s.total_rows(), Some(300));
    }

    #[test]
    fn copy_column_name_uses_selected_or_all_names() {
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;
        let result = super::super::super::state::QueryResultData {
            columns: vec![
                ColumnInfo {
                    name: "id".into(),
                    type_name: "int4".into(),
                    type_display: "int4".into(),
                    comment: None,
                },
                ColumnInfo {
                    name: "name".into(),
                    type_name: "text".into(),
                    type_display: "text".into(),
                    comment: None,
                },
            ],
            rows: vec![vec!["1".into(), "a".into()]],
            rows_affected: None,
            total_rows: None,
        };
        let state = ListState {
            result: Some(result.clone()),
            selected: true,
            col: 1,
            row: 0,
            ..ListState::default()
        };
        let (s, effects, dirty) = update(ListMessage::CopyColumnName, state);
        assert!(dirty);
        let copied = effects.iter().find_map(|e| match e {
            ResultsEffect::CopyColumnName { text } => Some(text.as_str()),
            _ => None,
        });
        assert_eq!(copied, Some("name"), "the selected column name is copied");
        assert_eq!(s.page_chord, None, "copy disarms a pending page chord");

        // Deselected (or a column index out of range): all names are copied.
        let state = ListState {
            result: Some(result),
            selected: false,
            ..ListState::default()
        };
        let (_s, effects, _d) = update(ListMessage::CopyColumnName, state);
        let copied = effects.iter().find_map(|e| match e {
            ResultsEffect::CopyColumnName { text } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(copied.as_deref(), Some("id, name"));
    }

    #[test]
    fn short_paginated_result_derives_total_from_row_count() {
        // A page that came back short of the page size is the whole result, so
        // its row count is already the total: no lazy `[c]count` affordance.
        let (s, _e, _d) = update(
            ListMessage::SetResult {
                result: super::super::super::state::QueryResultData {
                    columns: vec![crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo {
                        name: "x".into(),
                        type_name: "int4".into(),
                        type_display: "int4".into(),
                        comment: None,
                    }],
                    rows: vec![vec!["1".into()]; 25],
                    rows_affected: None,
                    total_rows: None,
                },
                paginated: true,
            },
            paginated_state(1, None, 100),
        );
        assert_eq!(
            s.total_rows(),
            Some(25),
            "a short page reveals the whole set"
        );
        assert!(s.at_last_page);
        assert!(
            !s.show_count_button(),
            "no count button when the total is known"
        );
        assert_eq!(s.total_cache, Some(25), "the derived total is cached");
    }

    #[test]
    fn later_page_short_result_infers_total_with_offset() {
        // Regression: a 999-row table on 500 rows/page shows page 2 with 499
        // rows. The short final page must infer the grand total as
        // `offset + row_count` (500 + 499), not the bare row count.
        let mut state = paginated_state(2, None, 499);
        state.row_limit = 500;
        let (s, _e, _d) = update(
            ListMessage::SetResult {
                result: super::super::super::state::QueryResultData {
                    columns: vec![crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo {
                        name: "x".into(),
                        type_name: "int4".into(),
                        type_display: "int4".into(),
                        comment: None,
                    }],
                    rows: vec![vec!["1".into()]; 499],
                    rows_affected: None,
                    total_rows: None,
                },
                paginated: true,
            },
            state,
        );
        assert_eq!(
            s.total_rows(),
            Some(999),
            "offset must be added to the tail page"
        );
        assert!(s.at_last_page);
        assert_eq!(s.total_cache, Some(999));
    }

    #[test]
    fn set_result_uses_bottom_anchor_and_sets_at_last_page() {
        // Prev nav left the bottom anchor armed; the landing result parks the
        // cursor on its last row instead of the top.
        let mut state = paginated_state(2, Some(300), 100);
        state.page_anchor_bottom = true;
        let (s, _e, _d) = update(
            ListMessage::SetResult {
                result: super::super::super::state::QueryResultData {
                    columns: state.result.clone().unwrap().columns,
                    rows: vec![vec!["1".into()]; 3],
                    rows_affected: None,
                    total_rows: Some(300),
                },
                paginated: true,
            },
            state,
        );
        assert_eq!(s.row, 2, "bottom-anchored landing selects the last row");
        assert!(s.selected);
        assert!(!s.page_anchor_bottom, "the anchor is consumed once");
        assert!(!s.at_last_page, "page 2 of 3 is not the last page");

        // Without a total a page returning fewer rows than the limit is the
        // last one, and an unanchored landing selects the first row.
        let (s, _e, _d) = update(
            ListMessage::SetResult {
                result: super::super::super::state::QueryResultData {
                    columns: vec![crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo {
                        name: "x".into(),
                        type_name: "int4".into(),
                        type_display: "int4".into(),
                        comment: None,
                    }],
                    rows: vec![vec!["1".into()]; 2],
                    rows_affected: None,
                    total_rows: None,
                },
                paginated: true,
            },
            paginated_state(1, None, 100),
        );
        assert!(
            s.at_last_page,
            "a short page with no total is the last page"
        );
        assert_eq!(s.row, 0, "a top-anchored landing selects the first row");
    }
}
