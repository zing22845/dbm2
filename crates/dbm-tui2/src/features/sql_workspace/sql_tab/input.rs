//! `sql_tab` hit-testing: where a click lands inside the SQL workspace.
//!
//! Translating a screen position into a [`SqlClickAction`] is input work, not
//! rendering: it reads the same layout the renderer draws
//! ([`super::layout::sql_tab_layout`]) and returns a message-ready action, so
//! the shell's mouse handlers never reach into the view for click semantics.

use ratatui::layout::Rect;

use super::editor::layout as editor_view;
use super::layout::sql_tab_layout;
use super::results::pagination::ResultsPaginationHit;
use super::session::{TabSession, session_view_key};
use super::state::{SqlFocus, SqlTabState};

/// The action produced by a mouse click inside the SQL workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlClickAction {
    /// Focus the clicked sub-pane (editor / history / results).
    FocusSubPane(SqlFocus),
    /// Activate the clicked tab, given its visible-tab offset.
    ActivateTab(usize),
    /// Close the open context picker (click outside it).
    CloseContextPicker,
    /// Click the editor header's context segment: open the context picker
    /// focused on `column` (the `· {db}` segment → Database, the `› {schema}`
    /// remainder → Schema).
    OpenContextPicker(
        crate::features::sql_workspace::sql_tab::editor::context_picker::state::PickerColumn,
    ),
    /// Click a picker row: move the cursor to that row (switching column);
    /// `double` additionally applies the selection.
    ContextPickerHit {
        column:
            crate::features::sql_workspace::sql_tab::editor::context_picker::state::PickerColumn,
        cursor: usize,
        double: bool,
    },
    /// Click inside a picker column area (not on a row): switch the focused column.
    ContextPickerColumn(
        crate::features::sql_workspace::sql_tab::editor::context_picker::state::PickerColumn,
    ),
    /// Double-click inside the history pane: apply the selected entry to the editor.
    HistoryApply,
    /// Single-click on a history list row: move cursor to that row index.
    HistoryRowClicked { index: usize },
    /// Click/drag on the history horizontal scrollbar.
    HistoryHScrollbar {
        track_x: u16,
        x: u16,
        max_scroll: usize,
        viewport_width: usize,
    },
    /// Click/drag on the history list vertical scrollbar.
    HistoryVScrollbar {
        track_y: u16,
        y: u16,
        max_scroll: usize,
        viewport_height: usize,
    },
    /// Single-click on a results list cell: move cursor to that `(row, col)`.
    ResultsCellClicked { row: usize, col: usize },
    /// Double-click inside the results pane: toggle the detail inspect mode.
    ResultsOpenDetail,
    /// Single-click on the results detail body while an edit session is active:
    /// focus the detail cell editor for the selected cell.
    ResultsFocusDetail,
    /// Click/drag on the results list horizontal scrollbar.
    ResultsHScrollbar {
        track_x: u16,
        x: u16,
        max_scroll: usize,
        viewport_width: usize,
    },
    /// Click/drag on the results list vertical scrollbar.
    ResultsVScrollbar {
        track_y: u16,
        y: u16,
        max_scroll: usize,
        viewport_height: usize,
    },
    /// Click the editor header's `· TblCmp:ON/OFF` chip to toggle table-name
    /// completion (equivalent to Alt+Tab in insert mode).
    ToggleTableCompletion,
    /// Click/drag on the editor body's vertical scrollbar.
    EditorVScrollbar {
        track_y: u16,
        y: u16,
        max_scroll: usize,
        viewport_height: usize,
    },
    /// Single click on a results column's header splitter: begin a drag to
    /// resize that column's width.
    ResultsColResize { col: usize },
    /// Single click on the results pagination toolbar (`« ‹ [p] › »` or the
    /// row-limit control). The hit rects mirror `layout_pagination_bar`.
    ResultsPagination { hit: ResultsPaginationHit },
    /// Single click on an enabled Results action-bar button (Refresh / Edit /
    /// Inst / Dup / Del / Commit / Rollback). Only produced when the button is
    /// active, mirroring `action_bar_button_at`.
    ResultsToolbar {
        action: crate::common::view::action_bar::ResultsAction,
    },
}

/// Hit-test a click at `(x, y)` inside the SQL workspace's tab-bar + body
/// region (`area`). A click on the top tab bar activates that tab; a click in
/// the body focuses the sub-pane (editor / history / results) under the cursor.
/// When the context picker is open, clicks on its rows/columns operate on it and
/// clicks outside it close it (mirroring the original dbm). Returns `None` for
/// clicks outside any interactive region (splitters, footer, empty areas).
pub fn sql_workspace_click(
    state: &SqlTabState,
    area: ratatui::layout::Rect,
    x: u16,
    y: u16,
    is_double_click: bool,
) -> Option<SqlClickAction> {
    use crate::features::sql_workspace::sql_tab::editor::context_picker::layout as cp_view;
    use crate::features::sql_workspace::sql_tab::editor::context_picker::state::PickerColumn;

    // The tab bar is the single top row; the child panes are below it.
    let tab_bar = ratatui::layout::Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    let body = ratatui::layout::Rect {
        x: area.x,
        y: area.y.saturating_add(1),
        width: area.width,
        height: area.height.saturating_sub(1),
    };

    // Click on the tab bar: activate that tab.
    if y == tab_bar.y {
        let sessions: Vec<TabSession> = state.tabs.iter().map(|t| t.session.clone()).collect();
        let visible = state.visible_tab_indices();
        return super::tab::tab_at(tab_bar, &sessions, &visible, x, y)
            .map(SqlClickAction::ActivateTab);
    }

    // Click in the body: focus the sub-pane under the cursor.
    let tab = state.active_tab()?;
    if body.width == 0 || body.height == 0 {
        return None;
    }
    let layout = sql_tab_layout(
        body,
        tab.splitter.editor_top_height,
        tab.splitter.history_pane_width,
    );
    if layout.editor.width == 0 {
        return None;
    }

    // When the picker is open, clicks inside it operate on it; clicks outside
    // it close it (the picker owns the editor sub-pane region).
    let picker_open = tab.editor.context_picker.open;
    if let Some(picker_area) = editor_view::context_picker_area(layout.editor, picker_open) {
        if contains(picker_area, x, y) {
            // A click on a visible row moves the cursor there (and, on a double
            // click, applies the selection).
            if let Some((column, cursor)) =
                cp_view::row_hit_at(picker_area, &tab.editor.context_picker, x, y)
            {
                return Some(SqlClickAction::ContextPickerHit {
                    column,
                    cursor,
                    double: is_double_click,
                });
            }
            // A click on a column's area (not a row) switches that column.
            let (db_rect, schema_rect) = cp_view::column_rects(picker_area);
            if contains(db_rect, x, y) {
                return Some(SqlClickAction::ContextPickerColumn(PickerColumn::Database));
            }
            if contains(schema_rect, x, y) {
                return Some(SqlClickAction::ContextPickerColumn(PickerColumn::Schema));
            }
            return None;
        }
        // A click in the SQL body outside the open picker closes it.
        return Some(SqlClickAction::CloseContextPicker);
    }

    // A click on the editor's title bar context segment opens the context
    // picker, focused on the column matching the clicked chip. The trigger is
    // always present (even without a chosen database), matching the original
    // dbm.
    if y == layout.editor.y {
        let (db_rect, full_rect) = editor_view::context_trigger_rects(
            layout.editor,
            tab.editor.editor.mode,
            tab.session.database.as_deref(),
            tab.session.schema.as_deref(),
        );
        if contains(full_rect, x, y) {
            let column = if contains(db_rect, x, y) {
                PickerColumn::Database
            } else {
                PickerColumn::Schema
            };
            return Some(SqlClickAction::OpenContextPicker(column));
        }
        // A click on the `· TblCmp:ON/OFF` chip in INSERT mode toggles
        // table-name completion (equivalent to Alt+Tab). Returns None outside
        // INSERT mode — the chip is not rendered then.
        if let Some(tblcmp) = editor_view::tblcmp_rect(
            layout.editor,
            tab.editor.editor.mode,
            tab.session.database.as_deref(),
            tab.session.schema.as_deref(),
            tab.editor.complete_table_names,
        ) && contains(tblcmp, x, y)
        {
            return Some(SqlClickAction::ToggleTableCompletion);
        }
    }
    // When the detail is visible it extends the history zone leftward (eating
    // into the editor). Clicks anywhere in that widened zone — including the
    // detail preview — must keep focus on History, not fall through to the
    // editor. Mirror the same zone computation used by the renderer.
    let (instance, connection) = session_view_key(&tab.session);
    let detail_visible = super::history::detail_visible(
        tab.focus == SqlFocus::History,
        &tab.history.list,
        &state.history_store,
        &instance,
        &connection,
    );
    // The History feature owns the list AND the detail; when the detail is
    // visible the history zone widens leftward (eating into the editor). Both
    // the shrunk editor and the widened history zone must be hit-tested so a
    // click on the detail keeps focus in History instead of falling to the
    // editor. Mirror the same geometry the renderer uses.
    let editor_hit;
    let history_hit;
    if detail_visible {
        let zone_x = super::history::splitter::layout::history_zone_x(
            body,
            &layout,
            tab.history.splitter.detail_pane_width,
        );
        let zone_w = super::history::splitter::layout::history_zone_width(
            &layout,
            tab.history.splitter.detail_pane_width,
        )
        .min(body.width);
        // Editor ends where the detail zone begins (shrunk, like the renderer).
        let shrunk_editor = Rect::new(
            layout.editor.x,
            layout.editor.y,
            zone_x
                .saturating_sub(layout.editor.x)
                .saturating_sub(layout.v_splitter.width)
                .max(1),
            layout.editor.height,
        );
        let history_zone = Rect::new(zone_x, layout.history.y, zone_w, layout.history.height);
        editor_hit = contains(shrunk_editor, x, y);
        history_hit = contains(history_zone, x, y);
    } else {
        editor_hit = contains(layout.editor, x, y);
        history_hit = contains(layout.history, x, y);
    }

    // Double-click inside the history pane applies the selected entry to the editor
    // (same as pressing Enter).
    if is_double_click && history_hit {
        return Some(SqlClickAction::HistoryApply);
    }

    // Single-click on a history list row: move the cursor to that row.
    // When History is not yet focused, the action handler will also switch
    // focus to History in the same step.
    if !is_double_click
        && history_hit
        && let Some(idx) = history_row_hit(state, tab, &layout, x, y, detail_visible)
    {
        return Some(SqlClickAction::HistoryRowClicked { index: idx });
    }

    // Click/drag on the history horizontal scrollbar.
    if !is_double_click
        && history_hit
        && let Some(action) = history_h_scrollbar_hit(state, tab, &layout, x, y, detail_visible)
    {
        return Some(action);
    }

    // Click/drag on the history vertical scrollbar.
    if !is_double_click
        && history_hit
        && let Some(action) = history_v_scrollbar_hit(state, tab, &layout, x, y, detail_visible)
    {
        return Some(action);
    }

    // —— Editor body vertical scrollbar hit-test ——
    // Use the same geometry function as `editor/view.rs::render` so the
    // scrollbar rect and max_scroll always match what is actually drawn.
    if !is_double_click && editor_hit {
        let (body_area, _footer_area, _picker_area) = editor_view::compute_editor_body_area(
            layout.editor,
            tab.editor.editor.mode,
            picker_open,
            tab.editor.complete_table_names,
            tab.editor.sql_search.text_input_active(),
            tab.editor.sql_search.has_filter(),
        );
        if let Some((v_bar, max_scroll)) =
            editor_view::editor_body_v_scrollbar_info(body_area, &tab.editor.editor)
            && crate::common::layout::pane_scrollbar::point_in_bar(v_bar, x, y)
        {
            return Some(SqlClickAction::EditorVScrollbar {
                track_y: v_bar.y,
                y,
                max_scroll,
                viewport_height: body_area.height.max(1) as usize,
            });
        }
    }

    // Results pane cell click handling (single-click: select cell; double-click: open detail).
    // The Results area has an outer Block with 1-col borders, so we first
    // compute the inner area, then delegate split logic to the splitter
    // sub-feature — only the list sub-pane responds to cell clicks.
    let results_hit = contains(layout.results, x, y);
    if results_hit {
        // Step 1: subtract outer Block borders (1 col on each side).
        let block_inner = ratatui::layout::Rect {
            x: layout.results.x.saturating_add(1),
            y: layout.results.y.saturating_add(1),
            width: layout.results.width.saturating_sub(2),
            height: layout.results.height.saturating_sub(2),
        };

        // Step 2: resolve the full Results layout (same single source of truth
        // the renderer uses). Only the content band is narrowed to the list side.
        let layout =
            crate::features::sql_workspace::sql_tab::results::layout::compute_results_layout(
                block_inner,
                tab.results.detail_open,
                tab.results.splitter.detail_pane_width,
                tab.results.list.row_count(),
                tab.results.list.search.text_input_active(),
                &tab.results.list.executed_sql_display(),
            );

        // Step 3: clicking an *enabled* Results action-bar button (Refresh /
        // Edit / Inst / Dup / Del / Commit / Rollback) acts on it. Disabled
        // buttons (and clicks on the bar's blank space) fall through.
        if let Some(action) =
            crate::features::sql_workspace::sql_tab::results::list::layout::action_bar_button_at(
                layout.list,
                &tab.results.list,
                x,
                y,
            )
        {
            return Some(SqlClickAction::ResultsToolbar { action });
        }

        // Step 4: clicking a pagination-toolbar control (page nav / row limit /
        // page number) acts on pagination, before the list cell handling below.
        if let Some(hit) = results_pagination_hit_at(tab, layout.pagination, x, y) {
            return Some(SqlClickAction::ResultsPagination { hit });
        }

        // Step 5: only the list sub-pane responds to cell clicks.
        if contains(layout.list, x, y)
            && tab.results.list.result.is_some()
            && tab.results.list.row_count() > 0
        {
            let table_area =
                crate::features::sql_workspace::sql_tab::results::list::layout::results_list_regions(
                    layout.list,
                )
                .1;

            if is_double_click && !tab.results.detail_open {
                return Some(SqlClickAction::ResultsOpenDetail);
            }

            // A single click on a results column's header splitter begins a
            // column-width resize drag (before the cell/scrollbar hit tests so
            // it takes precedence). Only the top header lines are resizable.
            if !is_double_click
                && let Some(col) =
                    crate::features::sql_workspace::sql_tab::results::list::layout::col_resize_hit_at(
                        layout.list,
                        &tab.results.list,
                        x,
                        y,
                    )
            {
                return Some(SqlClickAction::ResultsColResize { col });
            }

            // Check scrollbar hit BEFORE cell hit so scrollbar clicks take
            // precedence over cell clicks (scrollbar area is excluded from
            // cell_hit_at but we still want explicit scrollbar actions).
            if let Some(action) = results_v_scrollbar_hit(tab, table_area, x, y) {
                return Some(action);
            }
            if let Some(action) = results_h_scrollbar_hit(tab, table_area, x, y) {
                return Some(action);
            }

            if let Some((row, col)) =
                crate::features::sql_workspace::sql_tab::results::list::layout::cell_hit_at(
                    layout.list,
                    &tab.results.list,
                    x,
                    y,
                )
            {
                return Some(SqlClickAction::ResultsCellClicked { row, col });
            }
        }

        // A single click on the detail body while an edit session is active
        // focuses the detail cell editor for the selected cell (the table stays
        // focused otherwise, matching the read-only inspect detail).
        if !is_double_click
            && tab.results.list.edit.editing
            && !tab.results.detail.focused
            && let Some(detail_area) = layout.detail
            && contains(detail_area, x, y)
        {
            return Some(SqlClickAction::ResultsFocusDetail);
        }
        // Click in Block borders, splitter, or detail sub-pane → fall through
        // to focus-change logic below.
    }

    let focus = if editor_hit {
        SqlFocus::Editor
    } else if history_hit {
        SqlFocus::History
    } else if contains(layout.results, x, y) {
        SqlFocus::Results
    } else {
        return None; // splitter or footer
    };
    Some(SqlClickAction::FocusSubPane(focus))
}

/// Map a click inside the Results Block's inner area to a pagination-toolbar
/// hit, mirroring the toolbar geometry `results::view` renders (the
/// right-aligned `layout_pagination_bar` rects). Returns `None` for clicks
/// elsewhere, so only genuine toolbar controls act on pagination.
fn results_pagination_hit_at(
    tab: &crate::features::sql_workspace::sql_tab::state::SqlTab,
    pagination: Option<Rect>,
    x: u16,
    y: u16,
) -> Option<ResultsPaginationHit> {
    let list = &tab.results.list;
    let pag_area = pagination?;
    if list.row_count() == 0 {
        return None;
    }
    let total_rows = list.total_rows();
    let (counting, show_count) = list.toolbar_count_flags();
    let bar = crate::features::sql_workspace::sql_tab::results::pagination::layout_pagination_bar(
        pag_area,
        list.row_limit,
        list.page,
        total_rows,
        list.row_count(),
        counting,
        show_count,
    );
    for (hit, rect) in bar.hits {
        if contains(rect, x, y) {
            return Some(hit);
        }
    }
    None
}

fn contains(r: ratatui::layout::Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x.saturating_add(r.width) && y >= r.y && y < r.y.saturating_add(r.height)
}

/// Compute the results list sub-pane's inner rect (inside its Block borders,
/// after the detail/list split) for the given tab, mirroring exactly what
/// `sql_workspace_click` and the results render path use, so hover / drag
/// hit-testing agrees with the drawn geometry. `None` when the region is empty.
pub fn results_list_rect(
    tab: &crate::features::sql_workspace::sql_tab::state::SqlTab,
    area: ratatui::layout::Rect,
) -> Option<ratatui::layout::Rect> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let body = ratatui::layout::Rect {
        x: area.x,
        y: area.y.saturating_add(1),
        width: area.width,
        height: area.height.saturating_sub(1),
    };
    let layout = sql_tab_layout(
        body,
        tab.splitter.editor_top_height,
        tab.splitter.history_pane_width,
    );
    let results = layout.results;
    if results.width == 0 {
        return None;
    }
    let block_inner = ratatui::layout::Rect {
        x: results.x.saturating_add(1),
        y: results.y.saturating_add(1),
        width: results.width.saturating_sub(2),
        height: results.height.saturating_sub(2),
    };
    let layout = crate::features::sql_workspace::sql_tab::results::layout::compute_results_layout(
        block_inner,
        tab.results.detail_open,
        tab.results.splitter.detail_pane_width,
        tab.results.list.row_count(),
        tab.results.list.search.text_input_active(),
        &tab.results.list.executed_sql_display(),
    );
    Some(layout.list)
}

/// Try to hit-test a history list row at `(x, y)`. Returns the visible row
/// index if the click is on a list row, `None` otherwise.
fn history_row_hit(
    state: &SqlTabState,
    tab: &crate::features::sql_workspace::sql_tab::state::SqlTab,
    layout: &crate::features::sql_workspace::sql_tab::layout::SqlTabLayout,
    x: u16,
    y: u16,
    detail_visible: bool,
) -> Option<usize> {
    let (instance, connection) = session_view_key(&tab.session);
    let detail_w = if detail_visible {
        crate::features::sql_workspace::sql_tab::history::splitter::state::clamp_detail_pane_width(
            tab.history.splitter.detail_pane_width,
        )
    } else {
        0
    };

    // Compute the history zone (matching the geometry used by the renderer and
    // the hit-test above). When detail is visible the zone widens leftward.
    let history_zone = if detail_visible {
        let body_area = ratatui::layout::Rect {
            x: layout.editor.x,
            y: layout.editor.y.saturating_sub(1), // approximate — not exact
            width: layout.editor.width + 1 + layout.history.width,
            height: layout.editor.height,
        };
        let zone_x =
            crate::features::sql_workspace::sql_tab::history::splitter::layout::history_zone_x(
                body_area,
                layout,
                tab.history.splitter.detail_pane_width,
            );
        let zone_w =
            crate::features::sql_workspace::sql_tab::history::splitter::layout::history_zone_width(
                layout,
                tab.history.splitter.detail_pane_width,
            );
        ratatui::layout::Rect::new(zone_x, layout.history.y, zone_w, layout.history.height)
    } else {
        layout.history
    };

    // The history pane's inner area (minus border).
    let block = ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::ALL);
    let inner = block.inner(history_zone);

    // Compute the list footer height.
    let list_footer = crate::common::layout::hints::history_list_footer_text(
        tab.history.list.search.text_input_active(),
        tab.history.list.search.has_filter(),
        true,
    );
    let list_w = if detail_visible {
        history_zone
            .width
            .saturating_sub(detail_w)
            .saturating_sub(1) // detail + splitter
    } else {
        history_zone.width
    };
    let footer_h =
        crate::common::layout::text::footer_height(&list_footer, list_w.saturating_sub(2))
            .min(inner.height.saturating_sub(3));

    crate::features::sql_workspace::sql_tab::history::list::layout::row_hit_at(
        inner,
        &tab.history.list,
        &state.history_store,
        &instance,
        &connection,
        x,
        y,
        detail_visible,
        detail_w,
        footer_h,
    )
}

/// Check if a click at `(x, y)` hits the history list's horizontal scrollbar.
/// Returns the `HistoryHScrollbar` action with the scrollbar geometry if so.
#[allow(clippy::too_many_arguments)]
fn history_h_scrollbar_hit(
    state: &SqlTabState,
    tab: &crate::features::sql_workspace::sql_tab::state::SqlTab,
    layout: &crate::features::sql_workspace::sql_tab::layout::SqlTabLayout,
    x: u16,
    y: u16,
    detail_visible: bool,
) -> Option<SqlClickAction> {
    let (instance, connection) = session_view_key(&tab.session);
    let detail_w = if detail_visible {
        crate::features::sql_workspace::sql_tab::history::splitter::state::clamp_detail_pane_width(
            tab.history.splitter.detail_pane_width,
        )
    } else {
        0
    };

    let history_zone = if detail_visible {
        let body_area = ratatui::layout::Rect {
            x: layout.editor.x,
            y: layout.editor.y.saturating_sub(1),
            width: layout.editor.width + 1 + layout.history.width,
            height: layout.editor.height,
        };
        let zone_x =
            crate::features::sql_workspace::sql_tab::history::splitter::layout::history_zone_x(
                body_area,
                layout,
                tab.history.splitter.detail_pane_width,
            );
        let zone_w =
            crate::features::sql_workspace::sql_tab::history::splitter::layout::history_zone_width(
                layout,
                tab.history.splitter.detail_pane_width,
            );
        ratatui::layout::Rect::new(zone_x, layout.history.y, zone_w, layout.history.height)
    } else {
        layout.history
    };

    let block = ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::ALL);
    let inner = block.inner(history_zone);

    let list_footer = crate::common::layout::hints::history_list_footer_text(
        tab.history.list.search.text_input_active(),
        tab.history.list.search.has_filter(),
        true,
    );
    let list_w = if detail_visible {
        history_zone
            .width
            .saturating_sub(detail_w)
            .saturating_sub(1)
    } else {
        history_zone.width
    };
    let footer_h =
        crate::common::layout::text::footer_height(&list_footer, list_w.saturating_sub(2))
            .min(inner.height.saturating_sub(3));

    let list_area =
        crate::features::sql_workspace::sql_tab::history::list::layout::compute_list_area(
            inner,
            detail_visible,
            detail_w,
            footer_h,
        );

    let visible = tab
        .history
        .list
        .visible_indices(&state.history_store, &instance, &connection);
    let selected_width = tab
        .history
        .list
        .selected_entry(&state.history_store, &instance, &connection)
        .as_deref()
        .map(|sql| {
            crate::features::sql_workspace::sql_tab::history::store::history_line_display_width(sql)
                as usize
        })
        .unwrap_or(0);

    // Cut gutter off the left — scrollbar layout applies only to inner_content.
    let gutter_w = crate::common::components::line_numbers::gutter_width(visible.len());
    let inner_content = if list_area.width > gutter_w {
        ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([
                ratatui::layout::Constraint::Length(gutter_w),
                ratatui::layout::Constraint::Min(1),
            ])
            .split(list_area)[1]
    } else {
        list_area
    };

    let needs_h = selected_width > inner_content.width as usize;
    if !needs_h {
        return None;
    }

    // Compute h_scrollbar area using pane_scroll_layout to find its rect.
    let viewport_rows = inner_content.height as usize;
    let layout = crate::common::layout::pane_scrollbar::pane_scroll_layout(
        inner_content,
        selected_width as u16,
        visible.len(),
        viewport_rows.max(1),
    );
    let h_bar = layout.h_scrollbar?;

    if crate::common::layout::pane_scrollbar::point_in_bar(h_bar, x, y) {
        let content_w = layout.content_area.width as usize;
        let max_scroll = selected_width.saturating_sub(content_w);
        Some(SqlClickAction::HistoryHScrollbar {
            track_x: h_bar.x,
            x,
            max_scroll,
            viewport_width: content_w,
        })
    } else {
        None
    }
}

/// Returns the `HistoryVScrollbar` action with the scrollbar geometry if so.
#[allow(clippy::too_many_arguments)]
fn history_v_scrollbar_hit(
    state: &SqlTabState,
    tab: &crate::features::sql_workspace::sql_tab::state::SqlTab,
    layout: &crate::features::sql_workspace::sql_tab::layout::SqlTabLayout,
    x: u16,
    y: u16,
    detail_visible: bool,
) -> Option<SqlClickAction> {
    let (instance, connection) = session_view_key(&tab.session);
    let detail_w = if detail_visible {
        crate::features::sql_workspace::sql_tab::history::splitter::state::clamp_detail_pane_width(
            tab.history.splitter.detail_pane_width,
        )
    } else {
        0
    };

    let history_zone = if detail_visible {
        let body_area = ratatui::layout::Rect {
            x: layout.editor.x,
            y: layout.editor.y.saturating_sub(1),
            width: layout.editor.width + 1 + layout.history.width,
            height: layout.editor.height,
        };
        let zone_x =
            crate::features::sql_workspace::sql_tab::history::splitter::layout::history_zone_x(
                body_area,
                layout,
                tab.history.splitter.detail_pane_width,
            );
        let zone_w =
            crate::features::sql_workspace::sql_tab::history::splitter::layout::history_zone_width(
                layout,
                tab.history.splitter.detail_pane_width,
            );
        ratatui::layout::Rect::new(zone_x, layout.history.y, zone_w, layout.history.height)
    } else {
        layout.history
    };

    let block = ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::ALL);
    let inner = block.inner(history_zone);

    let list_footer = crate::common::layout::hints::history_list_footer_text(
        tab.history.list.search.text_input_active(),
        tab.history.list.search.has_filter(),
        true,
    );
    let list_w = if detail_visible {
        history_zone
            .width
            .saturating_sub(detail_w)
            .saturating_sub(1)
    } else {
        history_zone.width
    };
    let footer_h =
        crate::common::layout::text::footer_height(&list_footer, list_w.saturating_sub(2))
            .min(inner.height.saturating_sub(3));

    let list_area =
        crate::features::sql_workspace::sql_tab::history::list::layout::compute_list_area(
            inner,
            detail_visible,
            detail_w,
            footer_h,
        );

    let visible = tab
        .history
        .list
        .visible_indices(&state.history_store, &instance, &connection);
    if visible.is_empty() {
        return None;
    }

    // Cut gutter off the left — v_scrollbar sits on inner_content's right edge.
    let gutter_w = crate::common::components::line_numbers::gutter_width(visible.len());
    let inner_content = if list_area.width > gutter_w {
        ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([
                ratatui::layout::Constraint::Length(gutter_w),
                ratatui::layout::Constraint::Min(1),
            ])
            .split(list_area)[1]
    } else {
        list_area
    };

    // Compute selected-entry horizontal width so we know whether the renderer
    // also draws a horizontal scrollbar — that eats one row from the viewport.
    let selected_width = tab
        .history
        .list
        .selected_entry(&state.history_store, &instance, &connection)
        .as_deref()
        .map(|sql| {
            crate::features::sql_workspace::sql_tab::history::store::history_line_display_width(sql)
                as usize
        })
        .unwrap_or(0);

    let viewport_rows = inner_content.height as usize;
    let layout = crate::common::layout::pane_scrollbar::pane_scroll_layout(
        inner_content,
        selected_width as u16,
        visible.len(),
        viewport_rows.max(1),
    );
    let content_w = layout.content_area.width as usize;
    let needs_h = selected_width > content_w;
    let effective_layout = if needs_h {
        layout
    } else {
        crate::common::layout::pane_scrollbar::pane_scroll_layout(
            inner_content,
            0,
            visible.len(),
            viewport_rows.max(1),
        )
    };

    let v_bar = effective_layout.v_scrollbar?;
    // max_scroll is in content rows (matches render's data-rows calculation).
    // viewport_height is the TRACK'S PIXEL HEIGHT — drag formula uses it to
    // linearly map pointer Y (pixels) to scroll position.
    let content_rows = effective_layout.content_area.height.max(1) as usize;
    let max_scroll = visible.len().saturating_sub(content_rows);
    if max_scroll == 0 {
        return None;
    }

    if crate::common::layout::pane_scrollbar::point_in_bar(v_bar, x, y) {
        Some(SqlClickAction::HistoryVScrollbar {
            track_y: v_bar.y,
            y,
            max_scroll,
            viewport_height: usize::from(v_bar.height.max(1)),
        })
    } else {
        None
    }
}

/// Check if a click at `(x, y)` hits the results list vertical scrollbar.
/// `table_area` is pre-computed by [`results::list::view::results_list_regions`]
/// so both render and hit-test use identical geometry.
fn results_v_scrollbar_hit(
    tab: &crate::features::sql_workspace::sql_tab::state::SqlTab,
    table_area: Rect,
    x: u16,
    y: u16,
) -> Option<SqlClickAction> {
    let list = &tab.results.list;
    let result = list.result.as_ref()?;
    let row_count = result.rows.len();
    let table_width = crate::common::view::format::results_table_width(&list.col_widths);

    if !contains(table_area, x, y) {
        return None;
    }

    let layout = crate::common::layout::pane_scrollbar::pane_scroll_layout(
        table_area,
        table_width,
        row_count,
        table_area.height as usize,
    );
    let v_bar = layout.v_scrollbar?;
    // max_scroll is in DATA ROWS (each row is 2 pixels + 3-pixel header),
    // matching what render_table uses for scrollbar thumb sizing and anchor.
    let content_h = usize::from(layout.content_area.height.max(1));
    let visible_data_rows = if row_count > 0 {
        content_h.saturating_sub(usize::from(
            crate::common::view::format::RESULTS_HEADER_HEIGHT,
        )) / usize::from(crate::common::view::format::RESULTS_ROW_HEIGHT)
    } else {
        0
    };
    let max_scroll = row_count.saturating_sub(visible_data_rows.max(1));
    if max_scroll == 0 {
        return None;
    }

    if crate::common::layout::pane_scrollbar::point_in_bar(v_bar, x, y) {
        Some(SqlClickAction::ResultsVScrollbar {
            track_y: v_bar.y,
            y,
            max_scroll,
            // Track PIXEL height — drag formula needs this to linearly map
            // pointer Y (pixels) to scroll position. NOT the data-row count.
            viewport_height: usize::from(v_bar.height.max(1)),
        })
    } else {
        None
    }
}

/// Check if a click at `(x, y)` hits the results list horizontal scrollbar.
/// `table_area` is pre-computed by [`results::list::view::results_list_regions`]
/// so both render and hit-test use identical geometry.
fn results_h_scrollbar_hit(
    tab: &crate::features::sql_workspace::sql_tab::state::SqlTab,
    table_area: Rect,
    x: u16,
    y: u16,
) -> Option<SqlClickAction> {
    let list = &tab.results.list;
    let result = list.result.as_ref()?;
    let row_count = result.rows.len();
    let table_width = crate::common::view::format::results_table_width(&list.col_widths);

    if !contains(table_area, x, y) {
        return None;
    }

    let layout = crate::common::layout::pane_scrollbar::pane_scroll_layout(
        table_area,
        table_width,
        row_count,
        table_area.height as usize,
    );
    let h_bar = layout.h_scrollbar?;
    let viewport_width = layout.content_area.width.max(1) as usize;
    let max_scroll = (table_width as usize).saturating_sub(viewport_width);
    if max_scroll == 0 {
        return None;
    }

    if crate::common::layout::pane_scrollbar::point_in_bar(h_bar, x, y) {
        Some(SqlClickAction::ResultsHScrollbar {
            track_x: h_bar.x,
            x,
            max_scroll,
            viewport_width,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::super::history::splitter::layout::history_zone_width;
    use super::*;
    use ratatui::layout::Rect;

    fn state_with_tabs(count: usize) -> SqlTabState {
        let mut s = SqlTabState::default();
        for _ in 0..count {
            s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        }
        s
    }

    #[test]
    fn click_tab_bar_activates_tab() {
        let state = state_with_tabs(2);
        // area at x=0,y=0; tab bar is row 0. Click on the first tab.
        let area = Rect::new(0, 0, 80, 20);
        assert_eq!(
            sql_workspace_click(&state, area, 1, 0, false),
            Some(SqlClickAction::ActivateTab(0))
        );
        // Second tab is just past "<SQL 1> " (8 chars).
        assert_eq!(
            sql_workspace_click(&state, area, 9, 0, false),
            Some(SqlClickAction::ActivateTab(1))
        );
    }

    #[test]
    fn click_body_focuses_subpane() {
        let state = state_with_tabs(1);
        // Body is rows 1..; the editor occupies the left of the top row.
        let area = Rect::new(0, 0, 120, 40);
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(
            body,
            state.tabs[0].splitter.editor_top_height,
            state.tabs[0].splitter.history_pane_width,
        );
        // Click inside the editor region -> focus editor.
        let p = (layout.editor.x + 1, layout.editor.y + 1);
        assert_eq!(
            sql_workspace_click(&state, area, p.0, p.1, false),
            Some(SqlClickAction::FocusSubPane(SqlFocus::Editor))
        );
        // Click inside the history region -> focus history.
        let p = (layout.history.x + 1, layout.history.y + 1);
        assert_eq!(
            sql_workspace_click(&state, area, p.0, p.1, false),
            Some(SqlClickAction::FocusSubPane(SqlFocus::History))
        );
    }

    #[test]
    fn click_empty_or_splitter_returns_none() {
        let state = state_with_tabs(1);
        let area = Rect::new(0, 0, 120, 40);
        // Click on the splitter row between top row and results -> none.
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(
            body,
            state.tabs[0].splitter.editor_top_height,
            state.tabs[0].splitter.history_pane_width,
        );
        let p = (body.x + 1, layout.h_splitter.y);
        assert_eq!(sql_workspace_click(&state, area, p.0, p.1, false), None);
        // Click in the body below results (should be inside results actually);
        // instead assert a click outside the whole area returns none.
        assert_eq!(sql_workspace_click(&state, area, 500, 500, false), None);
    }

    #[test]
    fn click_header_trigger_opens_picker_focused_on_column() {
        use crate::features::sql_workspace::sql_tab::editor::context_picker::state::PickerColumn;
        let mut state = state_with_tabs(1);
        state.tabs[0].session.database = Some("mydb".into());
        state.tabs[0].session.schema = Some("public".into());
        let area = Rect::new(0, 0, 120, 40);
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(
            body,
            state.tabs[0].splitter.editor_top_height,
            state.tabs[0].splitter.history_pane_width,
        );
        // Click the `· mydb` segment -> focus Database column.
        let (db_rect, _full) = editor_view::context_trigger_rects(
            layout.editor,
            state.tabs[0].editor.editor.mode,
            Some("mydb"),
            Some("public"),
        );
        assert_eq!(
            sql_workspace_click(&state, area, db_rect.x + 1, db_rect.y, false),
            Some(SqlClickAction::OpenContextPicker(PickerColumn::Database))
        );
        // Click the `› public` remainder -> focus Schema column.
        let (_db, full) = editor_view::context_trigger_rects(
            layout.editor,
            state.tabs[0].editor.editor.mode,
            Some("mydb"),
            Some("public"),
        );
        assert_eq!(
            sql_workspace_click(&state, area, full.x + full.width - 1, full.y, false),
            Some(SqlClickAction::OpenContextPicker(PickerColumn::Schema))
        );
    }

    #[test]
    fn picker_row_click_and_column_click_and_outside_close() {
        use crate::features::sql_workspace::sql_tab::editor::context_picker::{
            layout as cp_view,
            state::{CachedList, ContextPickerState, PickerColumn},
        };
        let mut state = state_with_tabs(1);
        state.tabs[0].session.database = Some("mydb".into());
        state.tabs[0].session.schema = Some("public".into());
        state.tabs[0].editor.context_picker = ContextPickerState::open(
            PickerColumn::Database,
            "inst".into(),
            "conn".into(),
            "mydb".into(),
            "public".into(),
        );
        {
            let cp = &mut state.tabs[0].editor.context_picker;
            cp.databases = CachedList::Ready(vec!["a".into(), "b".into(), "c".into()]);
            cp.schemas = CachedList::Ready(vec!["public".into()]);
        }
        let area = Rect::new(0, 0, 120, 40);
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(
            body,
            state.tabs[0].splitter.editor_top_height,
            state.tabs[0].splitter.history_pane_width,
        );
        let picker_area = editor_view::context_picker_area(layout.editor, true).unwrap();
        let (db_rect, schema_rect) = cp_view::column_rects(picker_area);

        // Click a database row (inside the db column body) -> cursor hit.
        let row_y = db_rect.y + 1;
        let hit = sql_workspace_click(&state, area, db_rect.x + 2, row_y, false);
        assert!(matches!(
            hit,
            Some(SqlClickAction::ContextPickerHit {
                column: PickerColumn::Database,
                cursor,
                double: false
            }) if cursor == 0
        ));

        // Click the schema column area (its title row, not a row body) -> switch column.
        assert_eq!(
            sql_workspace_click(&state, area, schema_rect.x + 2, schema_rect.y, false),
            Some(SqlClickAction::ContextPickerColumn(PickerColumn::Schema))
        );

        // Click outside the picker (e.g. the history pane) -> close it.
        assert_eq!(
            sql_workspace_click(
                &state,
                area,
                layout.history.x + 1,
                layout.history.y + 1,
                false
            ),
            Some(SqlClickAction::CloseContextPicker)
        );
    }

    #[test]
    fn connection_a_open_picker_does_not_leak_into_connection_b() {
        use crate::features::sql_workspace::sql_tab::editor::context_picker::state::{
            ContextPickerState, PickerColumn,
        };
        // Two tabs: A (index 0) has its context picker left open; B (index 1)
        // is the active connection's tab and never opened a picker.
        let mut state = SqlTabState::default();
        state.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        state.open_connection_tab("inst".into(), "c2".into(), "id2".into(), None, None, None);
        state.tabs[0].session.database = Some("dbA".into());
        state.tabs[0].session.schema = Some("public".into());
        state.tabs[0].editor.context_picker = ContextPickerState::open(
            PickerColumn::Database,
            "inst".into(),
            "c1".into(),
            "dbA".into(),
            "public".into(),
        );
        state.tabs[1].session.database = Some("dbB".into());
        state.tabs[1].session.schema = Some("public".into());
        // B is active.
        state.active_tab = Some(1);

        let area = Rect::new(0, 0, 120, 40);
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(
            body,
            state.tabs[1].splitter.editor_top_height,
            state.tabs[1].splitter.history_pane_width,
        );

        // Clicking B's context trigger opens B's picker (B's picker is closed,
        // so this is NOT treated as an outside-click-close).
        let (_db, full) = editor_view::context_trigger_rects(
            layout.editor,
            state.tabs[1].editor.editor.mode,
            Some("dbB"),
            Some("public"),
        );
        assert_eq!(
            sql_workspace_click(&state, area, full.x + full.width - 1, full.y, false),
            Some(SqlClickAction::OpenContextPicker(PickerColumn::Schema))
        );

        // Clicking B's editor body focuses the editor (not CloseContextPicker,
        // because B's picker is closed).
        assert_eq!(
            sql_workspace_click(
                &state,
                area,
                layout.editor.x + 2,
                layout.editor.y + 3,
                false
            ),
            Some(SqlClickAction::FocusSubPane(SqlFocus::Editor))
        );
    }

    #[test]
    fn connection_without_database_can_still_open_context_picker() {
        use crate::features::sql_workspace::sql_tab::editor::context_picker::state::PickerColumn;
        // A connection whose tab has no database yet (opened straight from the
        // instances tree) must still be able to open its own context picker —
        // the picker's whole purpose is to choose a database. Previously the
        // header trigger was hidden when `database` was empty, so the click did
        // nothing and only connections that had already applied a context could
        // open a picker.
        let state = state_with_tabs(1);
        assert_eq!(state.tabs[0].session.database, None);
        let area = Rect::new(0, 0, 120, 40);
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(
            body,
            state.tabs[0].splitter.editor_top_height,
            state.tabs[0].splitter.history_pane_width,
        );
        let (_db, full) = editor_view::context_trigger_rects(
            layout.editor,
            state.tabs[0].editor.editor.mode,
            None,
            None,
        );
        // The `› …` remainder focuses the Schema column (database not chosen yet).
        assert_eq!(
            sql_workspace_click(&state, area, full.x + full.width - 1, full.y, false),
            Some(SqlClickAction::OpenContextPicker(PickerColumn::Schema))
        );
    }

    #[test]
    fn detail_expansion_eats_editor_not_list() {
        // Behavior 3: focusing History (detail open) keeps the list width fixed
        // and eats the editor width. Behavior 2: widening the detail shrinks the
        // editor (list unchanged) until the editor hits its minimum.
        use crate::features::sql_workspace::sql_tab::layout::sql_tab_layout;
        let area = Rect::new(0, 0, 120, 40);
        let layout = sql_tab_layout(area, 45, 30); // list = 30

        // Detail at 40 -> zone = list(30) + 40 + 1 + border 2 = 73 (list
        // unchanged, editor yields). Widening to 56 grows the zone further — the
        // list width (layout.history.width) is untouched; the editor absorbs the
        // growth.
        assert_eq!(history_zone_width(&layout, 40), 30 + 40 + 1 + 2);
        assert_eq!(history_zone_width(&layout, 56), 30 + 56 + 1 + 2);
        assert_eq!(layout.history.width, 30, "the list width must not change");
    }

    #[test]
    fn clicking_history_detail_keeps_focus_in_history() {
        let mut state = state_with_tabs(1);
        state.active_tab = Some(0);
        state.tabs[0].focus = crate::features::sql_workspace::sql_tab::state::SqlFocus::History;
        let (instance, connection) = session_view_key(&state.tabs[0].session);
        state
            .history_store
            .record_success(&instance, &connection, "SELECT * FROM users");

        let area = Rect::new(0, 0, 120, 40);
        let body = Rect::new(0, 1, 120, 39);
        let layout = sql_tab_layout(
            body,
            state.tabs[0].splitter.editor_top_height,
            state.tabs[0].splitter.history_pane_width,
        );
        // The detail zone extends left of `layout.history` (into what would be
        // the editor region). A click there must focus History, not the editor.
        let detail_w = state.tabs[0].history.splitter.detail_pane_width;
        let zone_w = (layout.history.width + detail_w + 1).min(body.width);
        let zone_x = body
            .x
            .max(layout.history.right().saturating_sub(zone_w))
            .min(body.right().saturating_sub(20));
        let detail_x = zone_x + 2; // inside the detail pane
        assert!(
            detail_x < layout.history.x,
            "detail must sit left of the base history list (test setup)"
        );
        let action = sql_workspace_click(&state, area, detail_x, layout.history.y + 2, false);
        assert!(
            matches!(
                action,
                Some(SqlClickAction::FocusSubPane(SqlFocus::History))
            ),
            "clicking the detail preview must keep focus in History, got {action:?}"
        );
    }

    #[test]
    fn double_click_history_row_returns_history_apply() {
        use crate::features::sql_workspace::sql_tab::layout::sql_tab_layout;

        let mut state = state_with_tabs(1);
        state.active_tab = Some(0);
        state
            .history_store
            .record_success("inst", "conn", "SELECT 1");
        state
            .history_store
            .record_success("inst", "conn", "SELECT 2");
        state
            .history_store
            .record_success("inst", "conn", "SELECT 3");
        let tab = &mut state.tabs[0];
        tab.focus = crate::features::sql_workspace::sql_tab::state::SqlFocus::History;
        tab.session.instance = Some("inst".to_string());
        tab.session.connection = Some("conn".to_string());
        tab.session.database = Some("postgres".to_string());

        let area = ratatui::layout::Rect::new(0, 0, 80, 20);
        let layout = sql_tab_layout(
            area,
            tab.splitter.editor_top_height,
            tab.splitter.history_pane_width,
        );

        // Click inside the history pane (the widened history zone when detail
        // is visible). Double-click should return HistoryApply.
        let click_x = layout.history.x + 2;
        let click_y = layout.history.y + 2;
        let action = sql_workspace_click(&state, area, click_x, click_y, true);
        assert!(
            matches!(action, Some(SqlClickAction::HistoryApply)),
            "double-click inside history pane must return HistoryApply, got {action:?}"
        );
    }

    #[test]
    fn single_click_history_row_focuses_pane() {
        use crate::features::sql_workspace::sql_tab::layout::sql_tab_layout;

        let mut state = state_with_tabs(1);
        state.active_tab = Some(0);
        state
            .history_store
            .record_success("inst", "conn", "SELECT 1");
        state
            .history_store
            .record_success("inst", "conn", "SELECT 2");
        let tab = &mut state.tabs[0];
        tab.focus = crate::features::sql_workspace::sql_tab::state::SqlFocus::History;
        tab.session.instance = Some("inst".to_string());
        tab.session.connection = Some("conn".to_string());

        let area = ratatui::layout::Rect::new(0, 0, 80, 20);
        let layout = sql_tab_layout(
            area,
            tab.splitter.editor_top_height,
            tab.splitter.history_pane_width,
        );

        // Single click (not double) should just focus the history pane.
        let click_x = layout.history.x + 2;
        let click_y = layout.history.y + 2;
        let action = sql_workspace_click(&state, area, click_x, click_y, false);
        assert!(
            matches!(
                action,
                Some(SqlClickAction::FocusSubPane(SqlFocus::History))
            ),
            "single click on history row must focus pane, got {action:?}"
        );
    }

    #[test]
    fn double_click_history_row_outside_list_does_not_apply() {
        use crate::features::sql_workspace::sql_tab::layout::sql_tab_layout;

        let mut state = state_with_tabs(1);
        state.active_tab = Some(0);
        state
            .history_store
            .record_success("inst", "conn", "SELECT 1");
        let tab = &mut state.tabs[0];
        tab.focus = crate::features::sql_workspace::sql_tab::state::SqlFocus::History;
        tab.session.instance = Some("inst".to_string());
        tab.session.connection = Some("conn".to_string());

        let area = ratatui::layout::Rect::new(0, 0, 80, 20);
        let layout = sql_tab_layout(
            area,
            tab.splitter.editor_top_height,
            tab.splitter.history_pane_width,
        );

        // Double-click outside the history pane (in the results pane).
        let click_x = layout.results.x + 2;
        let click_y = layout.results.y + 2;
        let action = sql_workspace_click(&state, area, click_x, click_y, true);
        assert!(
            !matches!(action, Some(SqlClickAction::HistoryApply)),
            "double-click outside history must not return HistoryApply"
        );
    }
}
