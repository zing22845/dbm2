//! SQL workspace key bindings, routed to the active tab's editor, results,
//! history and their overlays (context picker / completion popup).
//!
//! Part of the split keyboard layer; the entry points are
//! re-exported from the parent [`super`] module.

use super::nav::focus_subpane;
use crate::app::msg::AppMsg;
use crate::app::state::ModalKind;
use crate::features::sql_workspace::msg::{SqlMessage, SqlMsg};
use crate::features::sql_workspace::sql_tab::editor::context_picker::msg::{
    ContextPickerMessage, ContextPickerMsg,
};
use crate::features::sql_workspace::sql_tab::editor::context_picker::state::PickerColumn;
use crate::features::sql_workspace::sql_tab::editor::msg::{EditorMessage, EditorMsg};
use crate::features::sql_workspace::sql_tab::editor::sql_completion::msg::SqlCompletionMsg;
use crate::features::sql_workspace::sql_tab::history::msg::{HistoryMessage, HistoryMsg};
use crate::features::sql_workspace::sql_tab::msg::{SqlTabMessage, SqlTabMsg};
use crate::features::sql_workspace::sql_tab::results::msg::{
    ResultsMessage as SqlResultsMessage, ResultsMsg as SqlResultsMsg,
};
use crate::features::sql_workspace::sql_tab::results::pagination::{
    ResultsPageAction, ResultsPageScrollPlan, plan_results_page_scroll,
};
use crate::features::sql_workspace::sql_tab::state::SqlFocus;
use crate::features::sql_workspace::state::SqlState;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// SQL workspace key bindings, routed to the active tab's editor and its
/// overlays (context picker / completion popup).
pub(super) fn sql_key(key: KeyEvent, state: &SqlState) -> Option<AppMsg> {
    let tab = state.sql_tab.active_tab()?;
    let tab_id = tab.session.id;
    let editor = &tab.editor;
    // The detail cell editor captures everything while focused (whole-edit
    // session on, detail open): splitter nudges / height adjusts must not steal
    // `[`/`]`/`+`/`-` which are literal or vim keys there.
    let results_detail_focused = tab.focus
        == crate::features::sql_workspace::sql_tab::state::SqlFocus::Results
        && tab.results.detail.focused;
    tracing::debug!(
        code = ?key.code,
        modifiers = ?key.modifiers,
        focus = ?tab.focus,
        editor_mode = ?editor.editor.mode,
        "sql_key received"
    );

    // The context picker, when open, owns all keys — but only while the
    // editor pane is actually focused. If focus was moved away (e.g. via
    // mouse click to history) the picker must not keep intercepting keys.
    if tab.focus == SqlFocus::Editor && editor.context_picker.open {
        // While the picker's `/` search input is live every key feeds the
        // search handler (mirroring the pane searches): characters build the
        // query, Esc/Enter end it, Ctrl+p/n navigate and Ctrl+/ toggles case.
        // The case toggle also keeps working on an applied (visible) filter.
        let case_toggle = crate::common::components::search::is_case_toggle_key(&key);
        if editor.context_picker.search_input_active()
            || (case_toggle
                && (editor.context_picker.db_search.is_visible()
                    || editor.context_picker.schema_search.is_visible()))
        {
            return Some(sql_editor(
                EditorMessage::ContextPicker(ContextPickerMsg::Message(
                    ContextPickerMessage::SearchKey(key),
                )),
                tab_id,
            ));
        }
        return sql_context_picker_key(key, tab_id);
    }

    // The completion popup handles selection/apply/close when open. The key
    // mapping lives in the `sql_completion` feature (`key_to_msg`); the shell
    // only checks whether the popup is open and forwards a matching key.
    // Like the context picker, it must not capture keys when focus has left
    // the editor pane.
    if tab.focus == SqlFocus::Editor
        && editor.sql_completion.is_open()
        && let Some(msg) =
            crate::features::sql_workspace::sql_tab::editor::sql_completion::input::key_to_msg(key)
    {
        return Some(sql_editor(
            EditorMessage::SqlCompletion(SqlCompletionMsg::Message(msg)),
            tab_id,
        ));
    }

    // Note: the platform copy chord is *not* handled here. It is a global
    // shortcut — it must copy whichever editor holds a selection regardless of
    // the sub-pane focus — so it is resolved once in `key/dispatch.rs`
    // (`copy_selection_msg`), mirroring the original dbm's global
    // `copy_active_selection`.

    // Tab-bar / tab management keys (only when the popups are closed).
    if let Some(msg) = sql_tab_navigation_key(key, state) {
        return Some(msg);
    }

    // An active `/` search in the focused sub-pane consumes the pane's keys
    // (matching the original dbm): characters, Enter, Esc, Ctrl+/ and Ctrl+p/n
    // route to the search handler, and unmodified chrome keys such as the
    // splitter nudges (`[`/`]`, `+`/`-`) are suppressed. Tab management above
    // (Ctrl+W, Alt+1-9) is a modifier chord and still applies.
    //
    // The case toggle (`Ctrl+/`) additionally routes while the focused pane's
    // search is *visible* with an applied filter (input ended via Enter): the
    // filter stays shown on the bottom border, so toggling case must keep
    // working there too. Other keys are only consumed while actively typing.
    let case_toggle = crate::common::components::search::is_case_toggle_key(&key);
    match tab.focus {
        SqlFocus::History
            if tab.history.list.search.text_input_active()
                || (case_toggle && tab.history.list.search.is_visible()) =>
        {
            return Some(sql_history(tab_id, HistoryMessage::SearchKey(key)));
        }
        SqlFocus::Results
            if tab.results.list.search.text_input_active()
                || (case_toggle && tab.results.list.search.is_visible()) =>
        {
            return Some(sql_results(SqlResultsMessage::SearchKey(key), tab_id));
        }
        SqlFocus::Editor
            if tab.editor.sql_search.text_input_active()
                || (case_toggle && tab.editor.sql_search.search.is_visible()) =>
        {
            // The editor owns its in-buffer search (sql_search): forward the key
            // so the editor update routes it to the search input.
            return editor_key(key, tab_id);
        }
        _ => {}
    }

    // Vertical-splitter nudges resize the *vertical* splitter owned by the
    // focused sub-pane — matching the original dbm, which only moves a splitter
    // when the focused pane actually owns one:
    //   * Editor  -> the editor/history split (the history pane owns the right
    //                side, so `[` / `]` resize it);
    //   * History -> the internal detail/list split (the detail owns the left
    //                side: `]` grows it, `[` shrinks it);
    //   * Results -> the results detail/list split, and only while the detail
    //                pane is open. With the detail closed the results pane has
    //                no vertical splitter, so `[` / `]` are a no-op (they must
    //                NOT fall through to the history width as they used to).
    if !key.modifiers.contains(KeyModifiers::CONTROL) && !results_detail_focused {
        let nudge = match key.code {
            KeyCode::Char('[') => {
                Some(crate::common::layout::splitter::VerticalSplitterNudge::Left)
            }
            KeyCode::Char(']') => {
                Some(crate::common::layout::splitter::VerticalSplitterNudge::Right)
            }
            _ => None,
        };
        if let Some(nudge) = nudge {
            use crate::features::sql_workspace::sql_tab::state::SqlFocus;
            let msg = match tab.focus {
                SqlFocus::History => SqlTabMessage::NudgeHistoryDetailWidth { tab_id, nudge },
                SqlFocus::Results if tab.results.detail_open => {
                    SqlTabMessage::NudgeResultsDetailWidth { tab_id, nudge }
                }
                // Results with the detail closed: no vertical splitter to move.
                SqlFocus::Results => return None,
                SqlFocus::Editor => SqlTabMessage::NudgeHistoryWidth { tab_id, nudge },
            };
            return Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                SqlTabMsg::Message(msg),
            ))));
        }
    }

    // Horizontal-splitter adjust (`+` / `-`) works from any sub-pane: `+` grows
    // the currently-focused pane (top editor/history row or bottom results),
    // `-` shrinks it. In the editor's insert mode `+` / `-` are literal input,
    // so the adjust only applies otherwise.
    if !key.modifiers.contains(KeyModifiers::CONTROL) && !results_detail_focused {
        let editor_insert = tab.focus
            == crate::features::sql_workspace::sql_tab::state::SqlFocus::Editor
            && editor.editor.mode == edtui::EditorMode::Insert;
        let plus = match key.code {
            KeyCode::Char('+') => Some(true),
            KeyCode::Char('-') => Some(false),
            _ => None,
        };
        if let Some(plus) = plus
            && !editor_insert
        {
            return Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                SqlTabMsg::Message(SqlTabMessage::NudgeEditorTopHeight { tab_id, plus }),
            ))));
        }
    }

    // Route the key to the focused sub-pane.
    match tab.focus {
        SqlFocus::Editor => {
            // `,` opens the database/schema context picker in Normal/Visual mode,
            // matching the original dbm. The picker focuses the schema column.
            if key.code == KeyCode::Char(',')
                && key.modifiers.is_empty()
                && matches!(
                    tab.editor.editor.mode,
                    edtui::EditorMode::Normal | edtui::EditorMode::Visual
                )
            {
                return Some(sql_editor(
                    EditorMessage::ContextPicker(ContextPickerMsg::Message(
                        ContextPickerMessage::Open {
                            column: PickerColumn::Schema,
                            instance: tab.session.instance.clone().unwrap_or_default(),
                            connection: tab.session.connection.clone().unwrap_or_default(),
                            database: tab.session.database.clone().unwrap_or_default(),
                            schema: tab.session.schema.clone().unwrap_or_default(),
                        },
                    )),
                    tab_id,
                ));
            }
            // Alt+Enter runs the current editor SQL, matching the original
            // dbm (`run_sql_query`): it works in both Insert and Normal modes.
            // Ctrl+Enter was a redundant duplicate that many terminals fail to
            // report with the CONTROL modifier, so it has been removed.
            if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::ALT) {
                return Some(sql_editor(EditorMessage::Run, tab_id));
            }
            // Alt+Tab toggles table-name completion (TblCmp) in insert mode.
            // Ctrl+T is reserved for theme toggling in dbm2, so the original
            // dbm's Ctrl+T is rebound to Alt+Tab (the editor header shows the
            // TblCmp status).
            if key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::ALT) {
                return Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::ToggleTableCompletion { tab_id }),
                ))));
            }
            // `Shift+Tab` forces the completion popup open in insert mode,
            // matching the original dbm's `completion_trigger_key`: it accepts
            // either a `BackTab` code or a `Tab` + SHIFT combination (some
            // terminals report Shift+Tab as Tab+SHIFT), and always excludes
            // CONTROL/ALT so it doesn't collide with other shortcuts.
            // A tab-ish key code with the SHIFT modifier: some terminals report
            // Shift+Tab as `BackTab`, others as `Tab`+SHIFT or even `Char('\t')`.
            let is_tab_key = matches!(
                key.code,
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('\t')
            );
            let is_shift_tab = is_tab_key
                && key.modifiers.contains(KeyModifiers::SHIFT)
                && key
                    .modifiers
                    .intersection(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    .is_empty();
            tracing::debug!(
                is_shift_tab,
                insert_mode = matches!(tab.editor.editor.mode, edtui::EditorMode::Insert),
                "completion trigger check"
            );
            // The original dbm only triggers completion from insert mode.
            if is_shift_tab && matches!(tab.editor.editor.mode, edtui::EditorMode::Insert) {
                return Some(sql_editor(EditorMessage::ForceCompletion, tab_id));
            }
            // Otherwise forward the key to the editor buffer.
            editor_key(key, tab_id)
        }
        SqlFocus::Results => results_key(key, tab_id, &tab.results),
        SqlFocus::History => history_key(key, tab_id, &tab.history),
    }
}

/// Build a `SqlTabMessage::Results` targeting the given tab.
pub(super) fn sql_results(msg: SqlResultsMessage, tab_id: usize) -> AppMsg {
    AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
        SqlTabMessage::Results {
            tab_id,
            msg: SqlResultsMsg::Message(msg),
        },
    ))))
}

/// Results sub-pane keys: navigation, editing, and toolbar actions.
///
/// Move the results row selection by one (`down` true/false), crossing into the
/// next/previous SQL page at the edges. Shared by `j`/`k` and the arrows.
fn results_row_nav(
    list: &crate::features::sql_workspace::sql_tab::results::list::state::ListState,
    tab_id: usize,
    down: bool,
) -> Option<AppMsg> {
    if down {
        if list.row_count() > 0 && list.row + 1 >= list.row_count() && list.can_go_next_page() {
            Some(sql_results(
                SqlResultsMessage::PageNav {
                    action: ResultsPageAction::Next,
                },
                tab_id,
            ))
        } else {
            Some(sql_results(
                SqlResultsMessage::MoveSelection { dr: 1, dc: 0 },
                tab_id,
            ))
        }
    } else if list.row == 0 && list.can_go_prev_page() {
        Some(sql_results(
            SqlResultsMessage::PageNav {
                action: ResultsPageAction::Prev,
            },
            tab_id,
        ))
    } else {
        Some(sql_results(
            SqlResultsMessage::MoveSelection { dr: -1, dc: 0 },
            tab_id,
        ))
    }
}

/// Runs only when the active tab's sub-pane focus is `Results`, so keys here do
/// not collide with the editor's. Produces `ResultsMessage`s (or modal messages
/// via the returned `AppMsg`) and never mutates state directly.
fn results_key(
    key: KeyEvent,
    tab_id: usize,
    results: &crate::features::sql_workspace::sql_tab::results::state::ResultsState,
) -> Option<AppMsg> {
    // While the detail cell editor is focused the keys belong to it (edtui
    // Normal/Insert/Visual + Esc mode ladder), mirroring the original dbm's
    // `handle_results_detail_section_key`. This applies whether or not a
    // whole-result edit session is active: an editing key auto-starts the
    // session (see `ResultsMessage::DetailEditorKey`). Ctrl+S / Ctrl+U are the
    // single-cell Save / Discard here — the table's Commit / Rollback only
    // apply when the detail editor is not focused.
    if results.detail.focused && results.detail_open {
        return results_detail_editor_key(key, tab_id, results);
    }

    // Refresh re-runs the last query using the stored connection context.
    if key.code == KeyCode::Char('r') && key.modifiers.contains(KeyModifiers::CONTROL) {
        let needs = !results.list.last_sql.is_empty()
            && !results.list.last_instance.is_empty()
            && !results.list.last_connection.is_empty();
        return if needs {
            Some(sql_results(
                SqlResultsMessage::RunQuery {
                    instance: results.list.last_instance.clone(),
                    connection: results.list.last_connection.clone(),
                    database: results.list.last_database.clone(),
                    schema: results.list.last_schema.clone(),
                    sql: results.list.last_sql.clone(),
                    paginated: results.list.paginated,
                    page: results.list.page,
                    row_limit: results.list.row_limit,
                },
                tab_id,
            ))
        } else {
            None
        };
    }

    match key.code {
        // Enter: open (if needed) and focus the detail cell editor on the
        // selected cell — regardless of whether an edit session is active
        // (mirroring the original dbm's `open_results_cell_detail`). Browsing
        // keys work right away; the first editing key starts the session.
        KeyCode::Enter if key.modifiers.is_empty() => {
            Some(sql_results(SqlResultsMessage::FocusDetail, tab_id))
        }
        // Enter edit mode. Re-pressing `i` while a session is already active is
        // inert: a second EnterEdit would re-snapshot over the pending edits
        // and silently wipe them (cell editing happens in the detail editor,
        // where i/a/o enter Insert).
        KeyCode::Char('i') if key.modifiers.is_empty() => {
            if results.list.edit.editing {
                None
            } else {
                Some(sql_results(SqlResultsMessage::EnterEdit, tab_id))
            }
        }
        // ESC priority, mirroring the original dbm (Vim-like): clear the search
        // filter first, then exit edit mode, then close the detail, then
        // deselect the current cell. An active `/` search input is handled
        // earlier in `sql_key`, which routes every key (incl. Esc) to the search.
        KeyCode::Esc if key.modifiers.is_empty() => {
            if results.list.search.has_filter() {
                Some(sql_results(SqlResultsMessage::SearchKey(key), tab_id))
            } else if results.list.edit.editing {
                // Unsaved edits block the Esc exit (roll back or commit first);
                // a clean session exits as usual.
                if results.list.edit.is_dirty() {
                    Some(sql_results(SqlResultsMessage::EditLeaveAttempt, tab_id))
                } else {
                    Some(sql_results(SqlResultsMessage::ExitEdit, tab_id))
                }
            } else if results.detail_open {
                Some(sql_results(SqlResultsMessage::ToggleDetail, tab_id))
            } else {
                Some(sql_results(SqlResultsMessage::ResetSelection, tab_id))
            }
        }
        // Commit edits: open a preview modal with the built statements, then
        // `y`/`Enter` confirms and dispatches `Commit`.
        KeyCode::Char('s')
            if key.modifiers.contains(KeyModifiers::CONTROL) && results.list.edit.editing =>
        {
            let statements = results.list.build_commit_statements().ok();
            statements.map(|statements| {
                AppMsg::OpenModal(ModalKind::ResultsEditCommitPreview { statements })
            })
        }
        // Roll back edits.
        KeyCode::Char('u')
            if key.modifiers.contains(KeyModifiers::CONTROL) && results.list.edit.editing =>
        {
            Some(sql_results(SqlResultsMessage::Rollback, tab_id))
        }
        // Insert / duplicate / delete rows (edit mode only), matching the
        // original dbm's chords: Ctrl+i inserts a row, Ctrl+p duplicates the
        // selected row, and `d` `d` (two presses within 500 ms) deletes it — a
        // lone `d` only arms the chord (see `ListMessage::DelChord`).
        KeyCode::Char('i')
            if key.modifiers.contains(KeyModifiers::CONTROL) && results.list.edit.editing =>
        {
            Some(sql_results(SqlResultsMessage::AddRow, tab_id))
        }
        KeyCode::Char('p')
            if key.modifiers.contains(KeyModifiers::CONTROL) && results.list.edit.editing =>
        {
            Some(sql_results(SqlResultsMessage::DupRow, tab_id))
        }
        KeyCode::Char('d')
            if key.modifiers.is_empty() && results.list.edit.editing =>
        {
            Some(sql_results(SqlResultsMessage::DelChord, tab_id))
        }
        // Page navigation: `>` next page / `<` previous page, arming the
        // original dbm double-press chord — a second `>` within the chord
        // window jumps to the last page, a second `<` to the first. `<`/`>`
        // only exist directly on some keyboard layouts, so PageDown/PageUp
        // mirror the single-step nav below (they do not arm a chord).
        KeyCode::Char('>')
            if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            Some(sql_results(
                SqlResultsMessage::PageChord { forward: true },
                tab_id,
            ))
        }
        KeyCode::Char('<')
            if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            Some(sql_results(
                SqlResultsMessage::PageChord { forward: false },
                tab_id,
            ))
        }
        KeyCode::PageDown => Some(sql_results(
            SqlResultsMessage::PageNav {
                action: ResultsPageAction::Next,
            },
            tab_id,
        )),
        KeyCode::PageUp => Some(sql_results(
            SqlResultsMessage::PageNav {
                action: ResultsPageAction::Prev,
            },
            tab_id,
        )),
        // Cell selection via hjkl / arrows; on the last/first row of a page
        // they cross into the next/previous SQL page (mirroring the original
        // dbm's `move_results_row_selection`).
        KeyCode::Char('j') if key.modifiers.is_empty() => results_row_nav(&results.list, tab_id, true),
        KeyCode::Char('k') if key.modifiers.is_empty() => results_row_nav(&results.list, tab_id, false),
        KeyCode::Down => results_row_nav(&results.list, tab_id, true),
        KeyCode::Up => results_row_nav(&results.list, tab_id, false),
        // h/l move the cell selection horizontally, like the original dbm.
        KeyCode::Char('h') if key.modifiers.is_empty() => Some(sql_results(
            SqlResultsMessage::MoveSelection { dr: 0, dc: -1 },
            tab_id,
        )),
        KeyCode::Char('l') if key.modifiers.is_empty() => Some(sql_results(
            SqlResultsMessage::MoveSelection { dr: 0, dc: 1 },
            tab_id,
        )),
        KeyCode::Left => Some(sql_results(SqlResultsMessage::MoveSelection { dr: 0, dc: -1 }, tab_id)),
        KeyCode::Right => Some(sql_results(SqlResultsMessage::MoveSelection { dr: 0, dc: 1 }, tab_id)),
        // Copy the selected column name (`Ctrl+n`, original dbm binding). With
        // no cell selected the whole column-name list is copied.
        KeyCode::Char('n')
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && results.list.result.as_ref().is_some_and(|r| !r.columns.is_empty()) =>
        {
            Some(sql_results(SqlResultsMessage::CopyColumnName, tab_id))
        }
        // Flip one viewport inside the current page (`f` forward / `b` back,
        // vim-style); at the page edges the flip continues into the next /
        // previous SQL page (original dbm `page_scroll_results_selection`).
        KeyCode::Char(c)
            if (c.eq_ignore_ascii_case(&'f') || c.eq_ignore_ascii_case(&'b'))
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            results_flip(&results.list, tab_id, c.eq_ignore_ascii_case(&'f'))
        }
        // Jump to the first / last row of the current page (`g` / `G`).
        KeyCode::Char('g')
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            let list = &results.list;
            (list.row_count() > 0).then(|| {
                sql_results(
                    SqlResultsMessage::SetSelection {
                        row: 0,
                        col: list.col,
                    },
                    tab_id,
                )
            })
        }
        KeyCode::Char('G')
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            let list = &results.list;
            (list.row_count() > 0).then(|| {
                sql_results(
                    SqlResultsMessage::SetSelection {
                        row: list.row_count() - 1,
                        col: list.col,
                    },
                    tab_id,
                )
            })
        }
        // Jump to the next unsaved change while an edit session is active
        // (`m`, in Results-list focus): a pending insert / deleted row lands on
        // its row, an updated row on its first modified cell. Cycles forward
        // from the current row, wrapping to the first change.
        KeyCode::Char('m') if key.modifiers.is_empty() => results
            .list
            .next_change()
            .map(|(row, col)| sql_results(SqlResultsMessage::SetSelection { row, col }, tab_id)),
        // `,` / `.` narrow / widen the selected column (Vim-style), mirroring
        // the original dbm's column-width adjustment.
        KeyCode::Char(',') if key.modifiers.is_empty() => Some(sql_results(
            SqlResultsMessage::AdjustColWidth {
                delta: -crate::common::view::format::RESULTS_COL_WIDTH_STEP,
            },
            tab_id,
        )),
        KeyCode::Char('.') if key.modifiers.is_empty() => Some(sql_results(
            SqlResultsMessage::AdjustColWidth {
                delta: crate::common::view::format::RESULTS_COL_WIDTH_STEP,
            },
            tab_id,
        )),
        // Begin `/` search.
        KeyCode::Char('/') if key.modifiers.is_empty() => {
            Some(sql_results(SqlResultsMessage::BeginSearch, tab_id))
        }
        // `n` / `N` move to the next/previous match when a query filter is
        // applied (input already ended), matching the original dbm.
        KeyCode::Char(c)
            if c.eq_ignore_ascii_case(&'n')
                && results.list.search.has_filter()
                && !key.modifiers.contains(KeyModifiers::CONTROL)
                && crate::common::utils::shortcuts::pane_jump_modifiers_ok(key.modifiers) =>
        {
            use crate::common::utils::shortcuts::{
                caps_lock_active, effective_ascii_letter,
            };
            let forward = effective_ascii_letter(
                c,
                key.modifiers.contains(KeyModifiers::SHIFT),
                caps_lock_active(&key, false),
            ) == 'n';
            Some(sql_results(
                SqlResultsMessage::SearchNavigate { forward },
                tab_id,
            ))
        }
        // Count total rows (`c`, matching the `[c]count total rows` toolbar
        // control and the original dbm binding). Only offered while the last
        // query is count-able and its total is still unknown.
        KeyCode::Char('c')
            if key.modifiers.is_empty()
                && results.list.can_count_rows()
                && !results.list.counting =>
        {
            Some(sql_results(SqlResultsMessage::CountRows, tab_id))
        }
        // Rows-per-page picker modal (`r`, matching the `[r]rows` toolbar
        // label and the original dbm binding).
        KeyCode::Char('r') if key.modifiers.is_empty() => {
            Some(AppMsg::OpenModal(ModalKind::ResultsRowLimitPicker {
                current: results.list.row_limit,
                limits: crate::features::sql_workspace::sql_tab::results::pagination::RESULTS_ROW_LIMIT_PRESETS
                    .to_vec(),
            }))
        }
        // Page input modal (`p`, matching the `[p]` page-number toolbar label
        // and the original dbm binding).
        KeyCode::Char('p') if key.modifiers.is_empty() => {
            let total_pages = crate::features::sql_workspace::sql_tab::results::pagination::max_page(
                results.list.result.as_ref().and_then(|r| r.total_rows),
                results.list.row_limit,
            );
            Some(AppMsg::OpenModal(ModalKind::ResultsPageInput {
                current_page: results.list.page,
                total_pages,
                input: results.list.page.to_string(),
            }))
        }
        _ => {
            // Any *unbound* key between the two presses must break an armed
            // `dd` chord (a fast `d s d` must not delete): the chord is only
            // cleared by a message reaching the list update, so the key layer
            // turns an unmatched editing key into an explicit cancel.
            if results.list.edit.editing && results.list.del_chord_at.is_some() {
                Some(sql_results(SqlResultsMessage::DelChordCancel, tab_id))
            } else {
                None
            }
        }
    }
}

/// Keys while the detail cell editor is focused (detail open and
/// `detail.focused`; the whole-result edit session may or may not be active).
/// Mirrors the original dbm's `handle_results_detail_section_key`:
///   * Ctrl+S = save the draft to the cell, Ctrl+U = discard it (never the
///     table-wide Commit / Rollback, which only run while the table has focus);
///   * Esc lowers Insert/Visual to Normal first, then leaves the editor back to
///     the table;
///   * every other accepted key feeds the edtui editor. When no edit session is
///     active yet, an editing key auto-starts it in the results update (see
///     `ResultsMessage::DetailEditorKey`), so `i`/`a`/`o` just work.
fn results_detail_editor_key(
    key: KeyEvent,
    tab_id: usize,
    results: &crate::features::sql_workspace::sql_tab::results::state::ResultsState,
) -> Option<AppMsg> {
    let ctrl_only = key.modifiers.contains(KeyModifiers::CONTROL)
        && !key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::SHIFT);
    // Single-cell Save / Discard are detail-scoped: never fall through to the
    // table's Commit / Rollback.
    if ctrl_only && matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S')) {
        return Some(sql_results(SqlResultsMessage::SaveDetailCell, tab_id));
    }
    if ctrl_only && matches!(key.code, KeyCode::Char('u') | KeyCode::Char('U')) {
        return Some(sql_results(SqlResultsMessage::DiscardDetailCell, tab_id));
    }
    if key.code == KeyCode::Esc && key.modifiers.is_empty() {
        let mode = results
            .detail
            .editor
            .as_ref()
            .map(|host| host.editor.mode)
            .unwrap_or(edtui::EditorMode::Normal);
        // Esc in Insert/Visual first drops to Normal (stay in the editor); a
        // Normal-mode Esc leaves the detail editor back to the table.
        if mode == edtui::EditorMode::Normal {
            return Some(sql_results(SqlResultsMessage::UnfocusDetail, tab_id));
        }
        // Forward the Esc to the editor so edtui switches mode itself.
        return Some(sql_results(SqlResultsMessage::DetailEditorKey(key), tab_id));
    }
    // Everything edtui understands (typing, arrows, Enter, Tab…) routes to the
    // buffer. Unmapped chords are ignored rather than falling to the table.
    if crate::common::editor::accepts_key_event(&key) {
        return Some(sql_results(SqlResultsMessage::DetailEditorKey(key), tab_id));
    }
    None
}

/// Map a viewport flip (`f` / `b`) onto a results message: move the selection
/// by one viewport within the page, or cross into the next/previous SQL page
/// when the flip starts at the page edge (mirrors the original dbm
/// `page_scroll_results_selection`). `None` when there is nothing to flip.
fn results_flip(
    list: &crate::features::sql_workspace::sql_tab::results::list::state::ListState,
    tab_id: usize,
    forward: bool,
) -> Option<AppMsg> {
    let row_count = list.row_count();
    if row_count == 0 {
        return None;
    }
    let visible = list.viewport_rows.get().max(1);
    let plan = plan_results_page_scroll(
        list.row,
        row_count,
        visible,
        forward,
        list.paginated,
        list.page,
        list.can_go_next_page(),
    );
    let msg = match plan {
        ResultsPageScrollPlan::MoveToRow(row) => {
            SqlResultsMessage::SetSelection { row, col: list.col }
        }
        ResultsPageScrollPlan::NextPage => SqlResultsMessage::PageNav {
            action: ResultsPageAction::Next,
        },
        ResultsPageScrollPlan::PrevPage => SqlResultsMessage::PageNav {
            action: ResultsPageAction::Prev,
        },
        ResultsPageScrollPlan::NoPlan => return None,
    };
    Some(sql_results(msg, tab_id))
}

/// History sub-pane keys: navigation and apply. Runs only when sub-pane focus
/// is `History`. Produces `HistoryMessage`s without mutating state directly.
fn history_key(
    key: KeyEvent,
    tab_id: usize,
    history: &crate::features::sql_workspace::sql_tab::history::state::HistoryState,
) -> Option<AppMsg> {
    let search = &history.list.search;
    // ESC: if a search filter is set (but input not active), clear it first;
    // otherwise return focus to the SQL editor. (An active `/` search input is
    // handled earlier in `sql_key`, which routes every key to the search.)
    if key.code == KeyCode::Esc {
        if search.has_filter() {
            return Some(sql_history(tab_id, HistoryMessage::SearchKey(key)));
        }
        return Some(focus_subpane(SqlFocus::Editor));
    }
    let msg = match key.code {
        // j/k: up/down (global, not shown in footer)
        KeyCode::Char('j') if key.modifiers.is_empty() => HistoryMessage::MoveCursor { delta: 1 },
        KeyCode::Char('k') if key.modifiers.is_empty() => HistoryMessage::MoveCursor { delta: -1 },
        // h/l: horizontal scroll (global, not shown in footer)
        KeyCode::Char('h') if key.modifiers.is_empty() => {
            HistoryMessage::ScrollHScroll { delta: -1 }
        }
        KeyCode::Char('l') if key.modifiers.is_empty() => {
            HistoryMessage::ScrollHScroll { delta: 1 }
        }
        // Ctrl+p / Ctrl+n: up / down
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            HistoryMessage::MoveCursor { delta: -1 }
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            HistoryMessage::MoveCursor { delta: 1 }
        }
        // g / G: jump to top / bottom
        KeyCode::Char('g') if key.modifiers.is_empty() => HistoryMessage::SetCursor { index: 0 },
        // SHIFT is always set when typing uppercase 'G'; allow it, block
        // CONTROL/ALT so Ctrl+G / Alt+G don't trigger the jump.
        KeyCode::Char('G')
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            HistoryMessage::SetCursor { index: usize::MAX }
        }
        KeyCode::Up => HistoryMessage::MoveCursor { delta: -1 },
        KeyCode::Down => HistoryMessage::MoveCursor { delta: 1 },
        KeyCode::Left => HistoryMessage::ScrollHScroll { delta: -1 },
        KeyCode::Right => HistoryMessage::ScrollHScroll { delta: 1 },
        KeyCode::Enter => HistoryMessage::Apply,
        KeyCode::Char('/') if key.modifiers.is_empty() => HistoryMessage::BeginSearch,
        _ => return None,
    };
    Some(sql_history(tab_id, msg))
}

/// Build a `SqlTabMessage::History` app message targeting the given tab.
fn sql_history(tab_id: usize, msg: HistoryMessage) -> AppMsg {
    AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
        SqlTabMessage::History {
            tab_id,
            msg: HistoryMsg::Message(msg),
        },
    ))))
}

/// Tab-bar navigation keys: switch / open / close tabs.
fn sql_tab_navigation_key(key: KeyEvent, state: &SqlState) -> Option<AppMsg> {
    use crate::features::sql_workspace::sql_tab::msg::SqlTabMessage;
    // `Alt+t` opens a new tab and must work even with no tabs yet, so it is
    // handled before the empty guard below.
    if key.code == KeyCode::Char('t') && key.modifiers.contains(KeyModifiers::ALT) {
        return Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
            SqlTabMsg::Message(SqlTabMessage::OpenTab),
        ))));
    }
    let count = state.sql_tab.visible_tab_count();
    if count == 0 {
        return None;
    }
    let visible_active = state.sql_tab.global_to_visible().unwrap_or(0);
    let tab_msg = match key.code {
        // Plain `Shift+Tab` (BackTab) is not tab navigation — in the editor it
        // forces the completion popup open (see `sql_workspace_key`).
        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            SqlTabMessage::CloseTab(visible_active)
        }
        // `Alt+n` / `Alt+p` move to the next / previous visible tab.
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::ALT) => {
            SqlTabMessage::Tab((visible_active + 1) % count)
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::ALT) => {
            SqlTabMessage::Tab((visible_active + count - 1) % count)
        }
        // `Alt+1..9` jumps directly to the nth visible tab (1-based).
        KeyCode::Char(c)
            if key.modifiers.contains(KeyModifiers::ALT) && c.is_ascii_digit() && c != '0' =>
        {
            let idx = (c as u8 - b'1') as usize;
            if idx >= count {
                return None;
            }
            SqlTabMessage::Tab(idx)
        }
        _ => return None,
    };
    Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
        SqlTabMsg::Message(tab_msg),
    ))))
}

/// Keys for the context picker overlay (owns all keys while open).
fn sql_context_picker_key(key: KeyEvent, tab_id: usize) -> Option<AppMsg> {
    let msg = match key.code {
        KeyCode::Esc => ContextPickerMessage::Close,
        KeyCode::Tab => ContextPickerMessage::MoveColumn(PickerColumn::Schema),
        KeyCode::BackTab => ContextPickerMessage::MoveColumn(PickerColumn::Database),
        KeyCode::Enter => ContextPickerMessage::Apply,
        KeyCode::Up | KeyCode::Char('k') => ContextPickerMessage::MoveCursor { delta: -1 },
        KeyCode::Down | KeyCode::Char('j') => ContextPickerMessage::MoveCursor { delta: 1 },
        KeyCode::Left | KeyCode::Char('h') => {
            ContextPickerMessage::MoveColumn(PickerColumn::Database)
        }
        KeyCode::Right | KeyCode::Char('l') => {
            ContextPickerMessage::MoveColumn(PickerColumn::Schema)
        }
        KeyCode::Char('/') => ContextPickerMessage::BeginSearch,
        _ => return None,
    };
    Some(sql_editor(
        EditorMessage::ContextPicker(ContextPickerMsg::Message(msg)),
        tab_id,
    ))
}

/// Forward a key to the editor buffer (typing / navigation / modal commands).
fn editor_key(key: KeyEvent, tab_id: usize) -> Option<AppMsg> {
    Some(sql_editor(
        EditorMessage::KeyEvent {
            key,
            tracked_caps_lock: false,
        },
        tab_id,
    ))
}

/// Build an `AppMsg::Sql` message targeting the given tab's editor.
fn sql_editor(msg: EditorMessage, tab_id: usize) -> AppMsg {
    AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
        SqlTabMessage::Editor {
            tab_id,
            msg: EditorMsg::Message(msg),
        },
    ))))
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::app_shell::pane::Pane;

    use crate::app::key::key_to_msg;

    use crate::features::sql_workspace::sql_tab::state::SqlTabState;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    /// A `SqlState` with `count` tabs open. The default state opens no tabs, so
    /// we open `count` explicitly (for `count == 0`, an empty tab state).
    /// All tabs share the same connection so they are all visible.
    fn state_with_tabs(count: usize) -> SqlState {
        let mut tab_state = SqlTabState::default();
        for _ in 0..count {
            tab_state.open_connection_tab(
                "local".into(),
                "main-db".into(),
                "c1".into(),
                None,
                None,
                None,
            );
        }
        SqlState { sql_tab: tab_state }
    }

    /// An `AppState` with one open SQL tab (the default has none, since tabs
    /// are only created when a connection is selected). The tab is bound to a
    /// connection so it appears in the tab bar.
    fn app_state_with_tab() -> crate::app::state::AppState {
        let mut state = crate::app::state::AppState::default();
        state.sql.sql_tab.open_connection_tab(
            "local".into(),
            "main-db".into(),
            "c1".into(),
            None,
            None,
            None,
        );
        state
    }

    fn extract_tab_msg(msg: AppMsg) -> SqlTabMessage {
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(m)))) => m,
            _ => panic!("expected Sql tab message"),
        }
    }

    fn force_completion_msg(state: &SqlState, key: KeyEvent) -> AppMsg {
        sql_key(key, state).expect("Shift+Tab in insert-mode editor should force completion")
    }
    #[test]
    fn ctrl_tab_is_not_tab_navigation() {
        // `Ctrl+Tab` is no longer bound to tab switching; use `Alt+n` for the
        // next tab instead (there was never a footer hint advertising it).
        let state = state_with_tabs(3);
        assert!(
            sql_tab_navigation_key(key(KeyCode::Tab, KeyModifiers::CONTROL), &state).is_none(),
            "Ctrl+Tab must not switch tabs anymore"
        );
    }

    #[test]
    fn ctrl_shift_tab_is_not_tab_navigation() {
        // `Ctrl+Shift+Tab` is no longer bound to tab switching; use `Alt+p`
        // for the previous tab instead.
        let state = state_with_tabs(2);
        assert!(
            sql_tab_navigation_key(
                key(
                    KeyCode::BackTab,
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT
                ),
                &state,
            )
            .is_none(),
            "Ctrl+Shift+Tab must not switch tabs anymore"
        );
    }

    #[test]
    fn alt_p_switches_to_previous_tab() {
        let state = state_with_tabs(3);
        let count = state.sql_tab.visible_tab_count();
        let visible_active = state.sql_tab.global_to_visible().unwrap_or(0);
        assert_eq!(visible_active, 2);
        let msg = sql_tab_navigation_key(key(KeyCode::Char('p'), KeyModifiers::ALT), &state)
            .expect("alt+p should be handled");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Tab((visible_active + count - 1) % count)
        );
    }

    #[test]
    fn plain_shift_tab_is_not_tab_navigation() {
        // Plain Shift+Tab (BackTab without Ctrl) is no longer tab navigation:
        // in the editor it forces completion. So it must be None here.
        let state = state_with_tabs(2);
        assert!(
            sql_tab_navigation_key(key(KeyCode::BackTab, KeyModifiers::SHIFT), &state).is_none(),
            "plain Shift+Tab must not switch tabs"
        );
    }

    #[test]
    fn shift_tab_in_editor_forces_completion() {
        let mut state = state_with_tabs(1);
        // The completion trigger only fires in insert mode (original dbm).
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        let msg = force_completion_msg(&state, key(KeyCode::BackTab, KeyModifiers::SHIFT));
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg: EditorMsg::Message(EditorMessage::ForceCompletion),
                    ..
                },
            )))) => {}
            other => panic!("expected ForceCompletion, got {other:?}"),
        }
    }

    #[test]
    fn tab_plus_shift_also_forces_completion() {
        // Some terminals report Shift+Tab as `Tab` + SHIFT rather than BackTab;
        // both forms must trigger completion (original dbm).
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        let msg = force_completion_msg(&state, key(KeyCode::Tab, KeyModifiers::SHIFT));
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg: EditorMsg::Message(EditorMessage::ForceCompletion),
                    ..
                },
            )))) => {}
            other => panic!("expected ForceCompletion, got {other:?}"),
        }
    }

    #[test]
    fn char_tab_plus_shift_also_forces_completion() {
        // Some terminals report Shift+Tab as `Char('\t')` + SHIFT; that form
        // must also force completion in insert mode.
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        let msg = sql_key(key(KeyCode::Char('\t'), KeyModifiers::SHIFT), &state)
            .expect("char-tab+shift should force completion");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg: EditorMsg::Message(EditorMessage::ForceCompletion),
                    ..
                },
            )))) => {}
            other => panic!("expected ForceCompletion, got {other:?}"),
        }
    }

    #[test]
    fn shift_tab_in_normal_mode_does_not_force_completion() {
        // The original dbm only triggers completion from insert mode.
        let state = state_with_tabs(1); // default editor mode is normal
        let msg = sql_key(key(KeyCode::BackTab, KeyModifiers::SHIFT), &state);
        assert!(
            !matches!(
                msg,
                Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::Editor {
                        msg: EditorMsg::Message(EditorMessage::ForceCompletion),
                        ..
                    },)
                ))))
            ),
            "Shift+Tab in normal mode must not force completion"
        );
    }

    #[test]
    fn alt_tab_in_editor_emits_toggle_table_completion() {
        // Alt+Tab toggles TblCmp (Ctrl+T is reserved for theme toggling in
        // dbm2). In insert mode it must toggle table-name completion.
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        let msg = sql_key(key(KeyCode::Tab, KeyModifiers::ALT), &state)
            .expect("alt+tab should be handled in the editor");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::ToggleTableCompletion { .. },
            )))) => {}
            other => panic!("expected ToggleTableCompletion, got {other:?}"),
        }
    }

    #[test]
    fn jk_in_editor_with_completion_open_are_not_intercepted() {
        // With the completion popup open, `j`/`k` must NOT be consumed as
        // move-selection (the original dbm only binds the arrow keys): they fall
        // through so the user can type them into the buffer (e.g. "select 1 as ok").
        use crate::common::utils::cursor::Cursor;
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::CompletionItem;
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::CompletionKind;
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::state::SqlCompletionState;
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        state.sql_tab.tabs[0].editor.sql_completion = SqlCompletionState::open_with(
            vec![CompletionItem {
                label: "ok".into(),
                kind: CompletionKind::Keyword,
                detail: None,
                insert_text: "ok".into(),
            }],
            Cursor::new(0, 0),
            Cursor::new(0, 0),
        );
        // `k`/`j` must produce an editor KeyEvent (falls through), not a
        // MoveSelection.
        for code in [KeyCode::Char('k'), KeyCode::Char('j')] {
            let msg = sql_key(key(code, KeyModifiers::NONE), &state)
                .expect("j/k in an insert-mode editor with completion open must be handled");
            match msg {
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Editor {
                        msg: EditorMsg::Message(EditorMessage::KeyEvent { .. }),
                        ..
                    },
                )))) => {}
                other => panic!("expected editor KeyEvent for {code:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn arrow_keys_with_completion_open_move_selection() {
        // The arrow keys still move the completion selection when it is open.
        use crate::common::utils::cursor::Cursor;
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::msg::SqlCompletionMessage;
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::CompletionItem;
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::CompletionKind;
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::state::SqlCompletionState;
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        state.sql_tab.tabs[0].editor.sql_completion = SqlCompletionState::open_with(
            vec![CompletionItem {
                label: "ok".into(),
                kind: CompletionKind::Keyword,
                detail: None,
                insert_text: "ok".into(),
            }],
            Cursor::new(0, 0),
            Cursor::new(0, 0),
        );
        let msg = sql_key(key(KeyCode::Down, KeyModifiers::NONE), &state)
            .expect("Down with completion open should move selection");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg:
                        EditorMsg::Message(EditorMessage::SqlCompletion(SqlCompletionMsg::Message(
                            SqlCompletionMessage::MoveSelection { delta: 1 },
                        ))),
                    ..
                },
            )))) => {}
            other => panic!("expected MoveSelection for Down, got {other:?}"),
        }
    }

    #[test]
    fn comma_in_normal_mode_opens_the_context_picker() {
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Normal;
        let msg = sql_key(key(KeyCode::Char(','), KeyModifiers::NONE), &state)
            .expect("comma should open the context picker in normal mode");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor { .. },
            )))) => {}
            other => panic!("expected an editor message (context picker open), got {other:?}"),
        }
    }

    #[test]
    fn context_picker_search_active_routes_keys_to_search_handler() {
        let mut state = state_with_tabs(1);
        let tab = &mut state.sql_tab.tabs[0];
        tab.focus = SqlFocus::Editor;
        tab.editor.context_picker.open = true;
        // Begin the active-column `/` search (input live).
        tab.editor.context_picker.db_search.start();
        tab.editor.context_picker.db_search.query.push('a');

        // A character while the picker search is live must feed the search
        // handler instead of being dropped by the hardcoded picker bindings.
        let msg = sql_key(key(KeyCode::Char('b'), KeyModifiers::NONE), &state)
            .expect("character must route to the picker search while its input is active");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg:
                        EditorMsg::Message(EditorMessage::ContextPicker(ContextPickerMsg::Message(
                            ContextPickerMessage::SearchKey(_),
                        ))),
                    ..
                },
            )))) => {}
            other => panic!("expected ContextPicker SearchKey, got {other:?}"),
        }
    }

    #[test]
    fn context_picker_applied_filter_case_toggle_routes_to_search() {
        let mut state = state_with_tabs(1);
        let tab = &mut state.sql_tab.tabs[0];
        tab.focus = SqlFocus::Editor;
        tab.editor.context_picker.open = true;
        // Applied filter: query set, input ended via Enter.
        tab.editor.context_picker.db_search.query.push('a');
        assert!(tab.editor.context_picker.db_search.is_visible());
        assert!(!tab.editor.context_picker.search_input_active());

        // Ctrl+/ on an applied (visible) filter must still toggle case via the
        // search handler rather than falling through to the picker bindings.
        let msg = sql_key(key(KeyCode::Char('/'), KeyModifiers::CONTROL), &state)
            .expect("Ctrl+/ must route to the picker search while a filter is visible");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg:
                        EditorMsg::Message(EditorMessage::ContextPicker(ContextPickerMsg::Message(
                            ContextPickerMessage::SearchKey(_),
                        ))),
                    ..
                },
            )))) => {}
            other => panic!("expected ContextPicker SearchKey, got {other:?}"),
        }
    }

    #[test]
    fn alt_enter_in_normal_mode_runs_sql() {
        // Alt+Enter is the original dbm's run-SQL accelerator; it must work in
        // Normal mode without an open completion popup.
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Normal;
        let msg = sql_key(key(KeyCode::Enter, KeyModifiers::ALT), &state)
            .expect("alt+enter should run the editor SQL in normal mode");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg: EditorMsg::Message(EditorMessage::Run),
                    ..
                },
            )))) => {}
            other => panic!("expected EditorMessage::Run, got {other:?}"),
        }
    }

    #[test]
    fn alt_enter_in_insert_mode_runs_sql() {
        // Alt+Enter must also run SQL while the editor is in Insert mode.
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        let msg = sql_key(key(KeyCode::Enter, KeyModifiers::ALT), &state)
            .expect("alt+enter should run the editor SQL in insert mode");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg: EditorMsg::Message(EditorMessage::Run),
                    ..
                },
            )))) => {}
            other => panic!("expected EditorMessage::Run, got {other:?}"),
        }
    }

    #[test]
    fn alt_enter_with_completion_open_still_runs_sql() {
        // When the completion popup is open, a bare Enter applies the
        // completion, but Alt+Enter must fall through to run the SQL.
        use crate::common::utils::cursor::Cursor;
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::{
            CompletionItem, CompletionKind,
        };
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::state::SqlCompletionState;
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        state.sql_tab.tabs[0].editor.sql_completion = SqlCompletionState::open_with(
            vec![CompletionItem {
                label: "customers".into(),
                kind: CompletionKind::Table,
                detail: None,
                insert_text: "customers".into(),
            }],
            Cursor::new(0, 0),
            Cursor::new(0, 0),
        );
        let msg = sql_key(key(KeyCode::Enter, KeyModifiers::ALT), &state)
            .expect("alt+enter should run SQL even with the completion popup open");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg: EditorMsg::Message(EditorMessage::Run),
                    ..
                },
            )))) => {}
            other => panic!("expected EditorMessage::Run, got {other:?}"),
        }
    }

    #[test]
    fn tab_with_completion_open_applies_completion() {
        // When the completion popup is open, a bare Tab applies the highlighted
        // completion (matching the original dbm's `handle_popup_key`) instead
        // of inserting a tab character into the buffer.
        use crate::common::utils::cursor::Cursor;
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::msg::SqlCompletionMessage;
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::{
            CompletionItem, CompletionKind,
        };
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::state::SqlCompletionState;
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        state.sql_tab.tabs[0].editor.sql_completion = SqlCompletionState::open_with(
            vec![CompletionItem {
                label: "customers".into(),
                kind: CompletionKind::Table,
                detail: None,
                insert_text: "customers".into(),
            }],
            Cursor::new(0, 0),
            Cursor::new(0, 0),
        );
        let msg = sql_key(key(KeyCode::Tab, KeyModifiers::NONE), &state)
            .expect("bare tab with the completion popup open should apply completion");
        match msg {
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::Editor {
                    msg:
                        EditorMsg::Message(EditorMessage::SqlCompletion(SqlCompletionMsg::Message(
                            SqlCompletionMessage::Apply,
                        ))),
                    ..
                },
            )))) => {}
            other => panic!("expected SqlCompletionMessage::Apply, got {other:?}"),
        }
    }

    #[test]
    fn ctrl_w_closes_active_tab() {
        let state = state_with_tabs(2);
        let msg = sql_tab_navigation_key(key(KeyCode::Char('w'), KeyModifiers::CONTROL), &state)
            .expect("ctrl+w should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::CloseTab(1));
    }

    #[test]
    fn single_tab_alt_n_switching_wraps_to_itself() {
        // With a single tab, `Alt+n` (next tab) wraps back to the same tab.
        let state = state_with_tabs(1);
        let msg = sql_tab_navigation_key(key(KeyCode::Char('n'), KeyModifiers::ALT), &state)
            .expect("alt+n should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Tab(0));
    }

    #[test]
    fn alt_digit_switches_to_nth_tab() {
        let state = state_with_tabs(3); // 3 visible tabs
        // Alt+1 -> tab index 0, Alt+2 -> tab index 1, Alt+3 -> tab index 2.
        let msg = sql_tab_navigation_key(key(KeyCode::Char('1'), KeyModifiers::ALT), &state)
            .expect("alt+1 should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Tab(0));
        let msg = sql_tab_navigation_key(key(KeyCode::Char('3'), KeyModifiers::ALT), &state)
            .expect("alt+3 should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::Tab(2));
    }

    #[test]
    fn alt_digit_out_of_range_is_ignored() {
        let state = state_with_tabs(2); // only 2 tabs
        // Alt+9 exceeds the tab count, so it must not be handled.
        assert!(
            sql_tab_navigation_key(key(KeyCode::Char('9'), KeyModifiers::ALT), &state).is_none(),
            "alt+9 with only 2 tabs must be ignored"
        );
    }

    #[test]
    fn alt_t_opens_new_tab_even_without_tabs() {
        // `Alt+t` opens a tab and must work even when there are no tabs yet.
        let state = state_with_tabs(0);
        let msg = sql_tab_navigation_key(key(KeyCode::Char('t'), KeyModifiers::ALT), &state)
            .expect("alt+t should be handled");
        assert_eq!(extract_tab_msg(msg), SqlTabMessage::OpenTab);
    }

    #[test]
    fn alt_n_and_alt_p_cycle_tabs() {
        let state = state_with_tabs(3);
        let count = state.sql_tab.visible_tab_count();
        assert_eq!(count, 3);
        // The last opened tab is active, so `visible_active` is 2.
        let visible_active = state.sql_tab.global_to_visible().unwrap_or(0);
        assert_eq!(visible_active, 2);
        // Alt+n -> next tab (wrap from last back to first).
        let msg = sql_tab_navigation_key(key(KeyCode::Char('n'), KeyModifiers::ALT), &state)
            .expect("alt+n should be handled");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Tab((visible_active + 1) % count)
        );
        // Alt+p -> previous tab.
        let msg = sql_tab_navigation_key(key(KeyCode::Char('p'), KeyModifiers::ALT), &state)
            .expect("alt+p should be handled");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Tab((visible_active + count - 1) % count)
        );
    }

    #[test]
    fn results_key_i_enters_edit_mode() {
        let state = app_state_with_tab();
        let results = &state.sql.sql_tab.tabs[0].results;
        let msg = results_key(key(KeyCode::Char('i'), KeyModifiers::NONE), 0, results)
            .expect("i should enter edit mode");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                tab_id: 0,
                msg: SqlResultsMsg::Message(SqlResultsMessage::EnterEdit),
            }
        );
    }

    #[test]
    fn results_key_i_inert_while_edit_session_already_active() {
        // Re-pressing `i` inside an active session must not re-snapshot (that
        // would wipe pending edits) — it is a no-op on the table.
        let results = editing_results();
        assert!(
            results_key(key(KeyCode::Char('i'), KeyModifiers::NONE), 0, &results).is_none(),
            "i while already editing must not emit a second EnterEdit"
        );
    }

    #[test]
    fn results_key_ctrl_s_opens_commit_preview_only_when_editing() {
        // With no edit session, Ctrl+s yields no commit preview (no-op).
        let state = app_state_with_tab();
        let results = &state.sql.sql_tab.tabs[0].results;
        assert!(
            results_key(key(KeyCode::Char('s'), KeyModifiers::CONTROL), 0, results).is_none(),
            "ctrl+s with no edit session should be a no-op"
        );
    }

    /// A results snapshot with the whole-result edit session active.
    fn editing_results() -> crate::features::sql_workspace::sql_tab::results::state::ResultsState {
        let mut r =
            crate::features::sql_workspace::sql_tab::results::state::ResultsState::default();
        r.list.edit.editing = true;
        r
    }

    #[test]
    fn results_key_enter_editing_focuses_detail_not_toggle() {
        // In an edit session, Enter focuses the detail cell editor (the draft
        // + Ctrl+S/U model); it never toggles the read-only inspect pane.
        let results = editing_results();
        let msg = results_key(key(KeyCode::Enter, KeyModifiers::NONE), 0, &results)
            .expect("enter while editing must focus the detail editor");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                tab_id: 0,
                msg: SqlResultsMsg::Message(SqlResultsMessage::FocusDetail),
            }
        );
    }

    #[test]
    fn results_key_enter_without_edit_session_focuses_detail_editor() {
        // Outside an edit session Enter still opens + focuses the detail cell
        // editor (mirroring the original dbm's `open_results_cell_detail`);
        // browsing keys work right away and the first editing key auto-starts
        // the edit session. It never toggles only the read-only preview.
        let state = app_state_with_tab();
        let results = &state.sql.sql_tab.tabs[0].results;
        let msg = results_key(key(KeyCode::Enter, KeyModifiers::NONE), 0, results)
            .expect("enter outside an edit session must focus the detail editor");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                tab_id: 0,
                msg: SqlResultsMsg::Message(SqlResultsMessage::FocusDetail),
            }
        );
    }

    #[test]
    fn results_key_m_jumps_to_the_next_unsaved_change() {
        use crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo;
        use crate::features::sql_workspace::sql_tab::results::edit_sql::EditTarget;
        use crate::features::sql_workspace::sql_tab::results::state::{
            QueryResultData, ResultsState,
        };
        // One-column result; row 1 gets a pending update so `m` must land on it.
        let mut results = ResultsState::default();
        results.list.result = Some(QueryResultData {
            columns: vec![ColumnInfo {
                name: "a".into(),
                type_name: "text".into(),
                type_display: "text".into(),
                comment: None,
            }],
            rows: vec![vec!["1".into()], vec!["2".into()]],
            rows_affected: None,
            total_rows: Some(2),
        });
        results.list.row = 0;
        results.list.col = 0;
        results.list.edit_target = Some(EditTarget {
            schema: "public".into(),
            table: "t".into(),
            primary_keys: vec!["a".into()],
            columns: vec!["a".into()],
        });
        results.list.enter_edit();
        results.list.apply_cell_value(1, 0, "changed".into());
        let msg = results_key(key(KeyCode::Char('m'), KeyModifiers::NONE), 0, &results)
            .expect("m while editing must jump to the next change");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                tab_id: 0,
                msg: SqlResultsMsg::Message(SqlResultsMessage::SetSelection { row: 1, col: 0 }),
            }
        );
    }

    #[test]
    fn unbound_editing_key_cancels_an_armed_dd_chord() {
        // With a `d` armed, an otherwise-unmapped key (plain `s`) must cancel
        // the chord instead of silently passing — otherwise a fast `d s d`
        // would still delete.
        let mut results = editing_results();
        results.list.del_chord_at = Some(std::time::Instant::now());
        let msg = results_key(key(KeyCode::Char('s'), KeyModifiers::NONE), 0, &results)
            .expect("an unbound key while a chord is armed must cancel it");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                tab_id: 0,
                msg: SqlResultsMsg::Message(SqlResultsMessage::DelChordCancel),
            }
        );
        // Outside an edit session unbound keys stay a no-op.
        let clean =
            crate::features::sql_workspace::sql_tab::results::state::ResultsState::default();
        assert!(
            results_key(key(KeyCode::Char('s'), KeyModifiers::NONE), 0, &clean).is_none(),
            "an unbound key outside editing must not dispatch a cancel"
        );
    }

    #[test]
    fn focused_detail_ctrl_s_saves_cell_not_commits() {
        // While the detail editor is focused, Ctrl+S is the single-cell Save,
        // NOT the table-wide commit-preview modal.
        let mut results = editing_results();
        results.detail_open = true;
        results.detail.focused = true;
        let msg = results_key(key(KeyCode::Char('s'), KeyModifiers::CONTROL), 0, &results)
            .expect("ctrl+s while the detail editor is focused must save the cell");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                tab_id: 0,
                msg: SqlResultsMsg::Message(SqlResultsMessage::SaveDetailCell),
            }
        );
    }

    #[test]
    fn focused_detail_ctrl_u_discards_cell_not_rolls_back() {
        // Ctrl+U in the focused detail editor discards the single-cell draft;
        // the table-wide Rollback only applies while the detail is not focused.
        let mut results = editing_results();
        results.detail_open = true;
        results.detail.focused = true;
        let msg = results_key(key(KeyCode::Char('u'), KeyModifiers::CONTROL), 0, &results)
            .expect("ctrl+u while the detail editor is focused must discard the cell");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                tab_id: 0,
                msg: SqlResultsMsg::Message(SqlResultsMessage::DiscardDetailCell),
            }
        );
    }

    #[test]
    fn focused_detail_typed_key_routes_to_editor() {
        let mut results = editing_results();
        results.detail_open = true;
        results.detail.focused = true;
        // A printable key feeds the detail editor (draft), not the table.
        let msg = results_key(key(KeyCode::Char('x'), KeyModifiers::NONE), 0, &results)
            .expect("a typed key while focused must route to the editor");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                tab_id: 0,
                msg: SqlResultsMsg::Message(SqlResultsMessage::DetailEditorKey(key(
                    KeyCode::Char('x'),
                    KeyModifiers::NONE,
                ))),
            }
        );
    }

    #[test]
    fn focused_detail_esc_leaves_when_normal() {
        // With the detail editor at Normal mode (or no scratch editor yet), Esc
        // returns to the table (unfocus).
        let mut results = editing_results();
        results.detail_open = true;
        results.detail.focused = true;
        let msg = results_key(key(KeyCode::Esc, KeyModifiers::NONE), 0, &results)
            .expect("esc from a normal-mode detail editor must unfocus");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                tab_id: 0,
                msg: SqlResultsMsg::Message(SqlResultsMessage::UnfocusDetail),
            }
        );
    }

    #[test]
    fn results_esc_with_clean_edit_session_exits() {
        let results = editing_results(); // editing on, no changes yet
        let msg = results_key(key(KeyCode::Esc, KeyModifiers::NONE), 0, &results)
            .expect("esc with a clean edit session must exit it");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                tab_id: 0,
                msg: SqlResultsMsg::Message(SqlResultsMessage::ExitEdit),
            }
        );
    }

    #[test]
    fn results_esc_with_unsaved_edits_requests_the_leave_block() {
        // A dirty edit session must not silently drop its edits on Esc: the key
        // maps to the leave-attempt block (commit or roll back first).
        let mut results = editing_results();
        results.list.edit.enter_edit(&[vec!["1".into()]]);
        results.list.edit.apply_cell(0, 0, "2".into());
        assert!(results.list.edit.is_dirty());
        let msg = results_key(key(KeyCode::Esc, KeyModifiers::NONE), 0, &results)
            .expect("esc with unsaved edits must emit the leave-attempt block");
        assert_eq!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                tab_id: 0,
                msg: SqlResultsMsg::Message(SqlResultsMessage::EditLeaveAttempt),
            }
        );
    }

    #[test]
    fn focused_detail_splitter_keys_stay_editor_keys() {
        // `[` is not a splitter nudge while the detail editor is focused — it
        // is routed to the cell editor (vim/literal). Splitter handling lives in
        // sql_key, guarded by `results_detail_focused`.
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].focus = SqlFocus::Results;
        state.sql_tab.tabs[0].results.detail_open = true;
        state.sql_tab.tabs[0].results.list.edit.editing = true;
        state.sql_tab.tabs[0].results.detail.focused = true;
        let msg = sql_key(key(KeyCode::Char('['), KeyModifiers::NONE), &state)
            .expect("[ while the detail editor is focused must reach the results pane");
        assert!(
            matches!(
                msg,
                AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                    SqlTabMessage::Results { .. }
                ))))
            ),
            "[ while detail editor focused must route to results, got {msg:?}"
        );
    }

    #[test]
    fn uppercase_jumps_suppressed_when_history_search_active() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        state.sql.sql_tab.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        let tab = &mut state.sql.sql_tab.tabs[0];
        tab.focus = SqlFocus::History;
        tab.history.list.search.start();
        tab.history.list.search.query.push('a');
        // S while history search active must not produce a Focus(pane) jump.
        let msg = key_to_msg(key(KeyCode::Char('S'), KeyModifiers::SHIFT), &state);
        assert!(
            !matches!(
                msg,
                Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::Focus(_))
                ))))
            ),
            "S while history search active must not jump; got {msg:?}"
        );
        // Ctrl+/ while history search active must route to the search handler.
        let msg = key_to_msg(
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL),
            &state,
        );
        assert!(
            matches!(
                msg,
                Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::History { .. })
                ))))
            ),
            "Ctrl+/ while history search active must route to History SearchKey; got {msg:?}"
        );
    }

    #[test]
    fn jumps_and_case_toggle_apply_with_applied_history_filter() {
        // A filter stays visible after Enter ends the input (`active == false`):
        // `S`/`H`/`R` must still be suppressed and `Ctrl+/` must still toggle
        // case, so the search does not stop working once the query is applied.
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        state.sql.sql_tab.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        let tab = &mut state.sql.sql_tab.tabs[0];
        tab.focus = SqlFocus::History;
        // Applied filter: query set, input ended.
        tab.history.list.search.query.push('a');
        assert!(tab.history.list.search.has_filter());
        assert!(!tab.history.list.search.text_input_active());
        let msg = key_to_msg(key(KeyCode::Char('S'), KeyModifiers::SHIFT), &state);
        assert!(
            !matches!(
                msg,
                Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::Focus(_))
                ))))
            ),
            "S with an applied history filter must not jump; got {msg:?}"
        );
        let msg = key_to_msg(
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL),
            &state,
        );
        assert!(
            matches!(
                msg,
                Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::History { .. })
                ))))
            ),
            "Ctrl+/ with an applied history filter must route to History SearchKey; got {msg:?}"
        );
    }

    #[test]
    fn uppercase_jumps_suppressed_when_results_search_active() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        state.sql.sql_tab.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        let tab = &mut state.sql.sql_tab.tabs[0];
        tab.focus = SqlFocus::Results;
        tab.results.list.search.start();
        tab.results.list.search.query.push('a');
        let msg = key_to_msg(key(KeyCode::Char('H'), KeyModifiers::SHIFT), &state);
        assert!(
            !matches!(
                msg,
                Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::Focus(_))
                ))))
            ),
            "H while results search active must not jump; got {msg:?}"
        );
        let msg = key_to_msg(
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL),
            &state,
        );
        assert!(
            matches!(
                msg,
                Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::Results { .. })
                ))))
            ),
            "Ctrl+/ while results search active must route to Results SearchKey; got {msg:?}"
        );
    }

    #[test]
    fn uppercase_jumps_suppressed_when_editor_search_active() {
        let mut state = crate::app::state::AppState::default();
        state.focus = Pane::SQLWorkspace;
        state.sql.sql_tab.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        let tab = &mut state.sql.sql_tab.tabs[0];
        tab.focus = SqlFocus::Editor;
        tab.editor.sql_search.search.start();
        tab.editor.sql_search.search.query.push('a');
        let msg = key_to_msg(key(KeyCode::Char('R'), KeyModifiers::SHIFT), &state);
        assert!(
            !matches!(
                msg,
                Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::Focus(_))
                ))))
            ),
            "R while editor search active must not jump; got {msg:?}"
        );
        let msg = key_to_msg(
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL),
            &state,
        );
        assert!(
            matches!(
                msg,
                Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::Editor { .. })
                ))))
            ),
            "Ctrl+/ while editor search active must route to Editor; got {msg:?}"
        );
    }
    #[test]
    fn results_key_gt_and_lt_arm_page_chord() {
        let state = app_state_with_tab();
        let results = &state.sql.sql_tab.tabs[0].results;
        let next = results_key(key(KeyCode::Char('>'), KeyModifiers::NONE), 0, results)
            .expect("> should page forward (arming the chord)");
        assert_eq!(
            extract_tab_msg(next),
            SqlTabMessage::Results {
                tab_id: 0,
                msg: SqlResultsMsg::Message(SqlResultsMessage::PageChord { forward: true }),
            }
        );
        let prev = results_key(key(KeyCode::Char('<'), KeyModifiers::SHIFT), 0, results)
            .expect("< should page back (arming the chord)");
        assert_eq!(
            extract_tab_msg(prev),
            SqlTabMessage::Results {
                tab_id: 0,
                msg: SqlResultsMsg::Message(SqlResultsMessage::PageChord { forward: false }),
            }
        );
    }

    #[test]
    fn results_key_r_and_p_open_their_modals() {
        let state = app_state_with_tab();
        let results = &state.sql.sql_tab.tabs[0].results;
        let msg = results_key(key(KeyCode::Char('r'), KeyModifiers::NONE), 0, results)
            .expect("r should open the rows-per-page picker");
        assert!(
            matches!(
                msg,
                AppMsg::OpenModal(ModalKind::ResultsRowLimitPicker { .. })
            ),
            "r must open the row-limit picker, got {msg:?}"
        );
        let msg = results_key(key(KeyCode::Char('p'), KeyModifiers::NONE), 0, results)
            .expect("p should open the page input");
        assert!(
            matches!(msg, AppMsg::OpenModal(ModalKind::ResultsPageInput { .. })),
            "p must open the page input, got {msg:?}"
        );
    }

    #[test]
    fn results_key_c_counts_only_when_count_allowed() {
        let mut state = app_state_with_tab();
        {
            let tab = &mut state.sql.sql_tab.tabs[0];
            tab.results.list.paginated = true;
            tab.results.list.last_sql = "SELECT * FROM t".into();
            tab.results.list.result = Some(
                crate::features::sql_workspace::sql_tab::results::state::QueryResultData {
                    columns: Vec::new(),
                    rows: vec![Vec::new(); 100],
                    rows_affected: None,
                    total_rows: None,
                },
            );
        }
        // Count-able query with no total yet: `c` requests the count.
        let results = &state.sql.sql_tab.tabs[0].results;
        let msg = results_key(key(KeyCode::Char('c'), KeyModifiers::NONE), 0, results)
            .expect("c should request the total-row count");
        assert!(matches!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                msg: SqlResultsMsg::Message(SqlResultsMessage::CountRows),
                ..
            }
        ));

        // While a count is already running `c` is a no-op.
        let state = app_state_with_tab();
        let results = &state.sql.sql_tab.tabs[0].results;
        assert!(
            results_key(key(KeyCode::Char('c'), KeyModifiers::NONE), 0, results).is_none(),
            "c without a count-able result must be a no-op"
        );
    }

    fn filled_results_state() -> crate::app::state::AppState {
        let mut state = app_state_with_tab();
        let tab = &mut state.sql.sql_tab.tabs[0];
        tab.results.list.selected = true;
        tab.results.list.viewport_rows.set(10);
        tab.results.list.result =
            Some(crate::features::sql_workspace::sql_tab::results::state::QueryResultData {
                columns: vec![crate::features::sql_workspace::sql_tab::editor::sql_completion::provider::ColumnInfo {
                    name: "id".into(),
                    type_name: "int4".into(),
                    type_display: "int4".into(),
                    comment: None,
                }],
                rows: vec![vec!["x".into()]; 30],
                rows_affected: None,
                total_rows: None,
            });
        state
    }

    #[test]
    fn results_key_ctrl_n_copies_column_name() {
        let state = filled_results_state();
        let results = &state.sql.sql_tab.tabs[0].results;
        let msg = results_key(key(KeyCode::Char('n'), KeyModifiers::CONTROL), 0, results)
            .expect("Ctrl+n should copy the column name");
        assert!(matches!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                msg: SqlResultsMsg::Message(SqlResultsMessage::CopyColumnName),
                ..
            }
        ));
    }

    #[test]
    fn results_key_f_flips_by_one_viewport() {
        let state = filled_results_state();
        let results = &state.sql.sql_tab.tabs[0].results;
        // Row 0, viewport 10: `f` moves the selection down 10 rows.
        let msg = results_key(key(KeyCode::Char('f'), KeyModifiers::NONE), 0, results)
            .expect("f should flip forward one viewport");
        assert!(matches!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                msg: SqlResultsMsg::Message(SqlResultsMessage::SetSelection { row: 10, .. }),
                ..
            }
        ));
        // `b` from row 0 stays put (nothing above to flip to).
        let msg = results_key(key(KeyCode::Char('b'), KeyModifiers::NONE), 0, results);
        assert!(msg.is_none(), "b at the first row is a no-op");
    }

    #[test]
    fn results_key_g_and_caps_g_jump_page_ends() {
        let state = filled_results_state();
        let results = &state.sql.sql_tab.tabs[0].results;
        let msg = results_key(key(KeyCode::Char('G'), KeyModifiers::SHIFT), 0, results)
            .expect("G should jump to the last row");
        assert!(matches!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                msg: SqlResultsMsg::Message(SqlResultsMessage::SetSelection { row: 29, .. }),
                ..
            }
        ));
        let msg = results_key(key(KeyCode::Char('g'), KeyModifiers::NONE), 0, results)
            .expect("g should jump to the first row");
        assert!(matches!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                msg: SqlResultsMsg::Message(SqlResultsMessage::SetSelection { row: 0, .. }),
                ..
            }
        ));
    }

    #[test]
    fn results_key_pageup_pagedown_alias_page_nav() {
        let state = app_state_with_tab();
        let results = &state.sql.sql_tab.tabs[0].results;
        let msg = results_key(key(KeyCode::PageDown, KeyModifiers::NONE), 0, results)
            .expect("PageDown should page forward");
        assert!(matches!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                msg: SqlResultsMsg::Message(SqlResultsMessage::PageNav {
                    action: ResultsPageAction::Next,
                }),
                ..
            }
        ));
    }

    #[test]
    fn results_key_down_on_last_row_crosses_to_next_page() {
        let mut state = app_state_with_tab();
        // Paginated page 1 of 2 (100 rows per page, 200 total) with the
        // cursor on the last row: Down must page forward, not clamp.
        {
            let tab = &mut state.sql.sql_tab.tabs[0];
            tab.results.list.paginated = true;
            tab.results.list.page = 1;
            tab.results.list.row_limit = 100;
            tab.results.list.result = Some(
                crate::features::sql_workspace::sql_tab::results::state::QueryResultData {
                    columns: Vec::new(),
                    rows: vec![Vec::new(); 100],
                    rows_affected: None,
                    total_rows: Some(200),
                },
            );
            tab.results.list.row = 99;
        }
        let results = &state.sql.sql_tab.tabs[0].results;
        let msg = results_key(key(KeyCode::Down, KeyModifiers::NONE), 0, results)
            .expect("Down on the last row should navigate to the next page");
        assert!(matches!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                msg: SqlResultsMsg::Message(SqlResultsMessage::PageNav {
                    action: ResultsPageAction::Next,
                }),
                ..
            }
        ));

        // On a middle row, Down stays a plain cell move.
        let state = app_state_with_tab();
        let results = &state.sql.sql_tab.tabs[0].results;
        let msg = results_key(key(KeyCode::Down, KeyModifiers::NONE), 0, results)
            .expect("Down should move the cell selection");
        assert!(matches!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                msg: SqlResultsMsg::Message(SqlResultsMessage::MoveSelection { .. }),
                ..
            }
        ));
    }

    #[test]
    fn results_key_up_on_first_row_crosses_to_prev_page() {
        let mut state = app_state_with_tab();
        {
            let tab = &mut state.sql.sql_tab.tabs[0];
            tab.results.list.paginated = true;
            tab.results.list.page = 2;
            tab.results.list.row_limit = 100;
            tab.results.list.row = 0;
        }
        let results = &state.sql.sql_tab.tabs[0].results;
        let msg = results_key(key(KeyCode::Up, KeyModifiers::NONE), 0, results)
            .expect("Up on the first row of a later page should go back a page");
        assert!(matches!(
            extract_tab_msg(msg),
            SqlTabMessage::Results {
                msg: SqlResultsMsg::Message(SqlResultsMessage::PageNav {
                    action: ResultsPageAction::Prev,
                }),
                ..
            }
        ));
    }

    #[test]
    fn sql_key_does_not_claim_the_copy_chord() {
        // The copy chord is global (`key/dispatch::copy_selection_msg`), so the
        // workspace's own key map must never produce it — otherwise the same
        // chord would have two owners.
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].focus = SqlFocus::Editor;
        state.sql_tab.tabs[0].editor.editor.selection = Some(edtui::Selection::new(
            edtui::Index2::new(0, 1),
            edtui::Index2::new(0, 3),
        ));
        #[cfg(not(target_os = "macos"))]
        let copy_key = key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        #[cfg(target_os = "macos")]
        let copy_key = key(KeyCode::Char('c'), KeyModifiers::SUPER);
        let msg = sql_key(copy_key, &state);
        assert!(
            !matches!(
                msg,
                Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::Editor {
                        msg: EditorMsg::Message(EditorMessage::CopySelection),
                        ..
                    },)
                ))))
            ),
            "the copy chord belongs to the global gate, not to sql_key"
        );
    }

    #[test]
    fn bracket_nudges_the_vertical_splitter_of_the_focused_subpane() {
        use crate::common::layout::splitter::VerticalSplitterNudge as N;

        // History focused: the internal detail/list splitter (detail on the left).
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].focus = SqlFocus::History;
        let msg = sql_key(key(KeyCode::Char(']'), KeyModifiers::NONE), &state)
            .expect("] must nudge the history detail split");
        assert!(matches!(
            msg,
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::NudgeHistoryDetailWidth {
                    nudge: N::Right,
                    ..
                }
            ))))
        ));

        // Results focused with the detail open: the results detail split.
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].focus = SqlFocus::Results;
        state.sql_tab.tabs[0].results.detail_open = true;
        let msg = sql_key(key(KeyCode::Char('['), KeyModifiers::NONE), &state)
            .expect("[ must nudge the results detail split");
        assert!(matches!(
            msg,
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::NudgeResultsDetailWidth { nudge: N::Left, .. }
            ))))
        ));

        // Results focused with the detail CLOSED: no vertical splitter to move,
        // so the key is a no-op (it must not resize the history pane).
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].focus = SqlFocus::Results;
        state.sql_tab.tabs[0].results.detail_open = false;
        for code in [KeyCode::Char('['), KeyCode::Char(']')] {
            let msg = sql_key(key(code, KeyModifiers::NONE), &state);
            assert!(
                msg.is_none(),
                "[ / ] with the results detail closed must do nothing, got {msg:?}"
            );
        }

        // Editor focused: the editor/history split (history owns the right side).
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].focus = SqlFocus::Editor;
        let msg = sql_key(key(KeyCode::Char(']'), KeyModifiers::NONE), &state)
            .expect("] must nudge the history pane width");
        assert!(matches!(
            msg,
            AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(SqlTabMsg::Message(
                SqlTabMessage::NudgeHistoryWidth {
                    nudge: N::Right,
                    ..
                }
            ))))
        ));
    }

    #[test]
    fn plain_c_in_editor_is_not_a_copy() {
        // An unmodified `c` must stay editor input (insert a char in insert
        // mode / vim motion prefix in normal mode), never trigger copy.
        let mut state = state_with_tabs(1);
        state.sql_tab.tabs[0].focus = SqlFocus::Editor;
        let msg = sql_key(key(KeyCode::Char('c'), KeyModifiers::NONE), &state);
        assert!(
            !matches!(
                msg,
                Some(AppMsg::Sql(SqlMsg::Message(SqlMessage::SqlTab(
                    SqlTabMsg::Message(SqlTabMessage::Editor {
                        msg: EditorMsg::Message(EditorMessage::CopySelection),
                        ..
                    },)
                ))))
            ),
            "plain c must not copy"
        );
    }
}
