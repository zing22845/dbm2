//! Geometry and hit-testing for mouse input.
//!
//! Every rect the mouse handlers compare against is computed here, from the
//! same layout functions the renderer uses — so a splitter can only ever be
//! found where it is actually drawn.
//!
//! This module is deliberately free of message dispatch: it answers "where is
//! X on screen", never "what should happen when X is clicked" (that is
//! `click.rs`).

use ratatui::layout::Rect;

use crate::app::state::AppState;
use crate::app_shell::pane::Pane;
use crate::features::global_footer::layout as footer_layout;
use crate::features::perf_monitor::view as perf_view;

/// Width of the global footer row's right perf strip (what the footer's
/// horizontal split reserves in `app::view`). Centralized here so the footer
/// height math and the actual perf slot never drift apart.
pub(crate) fn global_footer_perf_width(state: &AppState, width: u16) -> u16 {
    perf_view::perf_width(&state.perf).min(width / 3)
}

/// Height of the global footer row for `width` columns.
///
/// Mirrors the renderer: the hints render beside the perf readout, so size the
/// footer against `width - perf_w` — not the full width — or the wrapped hints
/// steal the status line's row and clip it off the bottom. Every layer that
/// reserves body space above the footer (render, hit-testing, splitter tracks)
/// must use this one function so their geometry stays identical.
pub(crate) fn global_footer_height(state: &AppState, width: u16) -> u16 {
    let hints_w = width.saturating_sub(global_footer_perf_width(state, width));
    footer_layout::footer_height(&state.footer, hints_w)
}

pub(crate) fn app_body_geometry(
    size: ratatui::layout::Size,
    body_top: u16,
    body_h: u16,
    state: &AppState,
) -> Option<crate::features::app_splitter::layout::AppBodyLayout> {
    let body_area = Rect::new(0, body_top, size.width, body_h);
    let layout = crate::features::app_splitter::layout::app_body_layout(
        body_area,
        state.splitter.explorer_pane_width,
    );
    (layout.workspace.width > 0 && layout.explorer.width > 0).then_some(layout)
}

/// The workspace region of the app body (right of the Explorer / workspace
/// splitter). Returns `None` when the body is too small to lay out both panes.
pub(crate) fn workspace_rect_for_hit(
    size: ratatui::layout::Size,
    body_top: u16,
    body_h: u16,
    state: &AppState,
) -> Option<ratatui::layout::Rect> {
    app_body_geometry(size, body_top, body_h, state).map(|g| g.workspace)
}

/// The Explorer column rect (the left pane of the app body splitter).
pub(crate) fn app_explorer_rect(
    size: ratatui::layout::Size,
    body_top: u16,
    body_h: u16,
    state: &AppState,
) -> Option<ratatui::layout::Rect> {
    app_body_geometry(size, body_top, body_h, state).map(|g| g.explorer)
}

/// Compute the SQL tab region (tab bar + child panes) for mouse hit-testing,
/// mirroring `sql_workspace/view.rs` (workspace inner minus its tab footer).
/// Returns `None` when the SQL workspace is not the region being shown.
pub(crate) fn sql_tab_area_for_hit(
    size: ratatui::layout::Size,
    state: &AppState,
) -> Option<ratatui::layout::Rect> {
    if state.modal.is_some()
        || matches!(state.focus, Pane::Discover(_))
        || state.instance_workspace_open()
        || state.sql.sql_tab.tabs.is_empty()
    {
        return None;
    }
    let footer_h = global_footer_height(state, size.width);
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    if body_h < 3 {
        return None;
    }
    let workspace = workspace_rect_for_hit(size, body_top, body_h, state)?;
    // Outer " SQL Workspace " border (1 col/row).
    let inner = Rect::new(
        workspace.x.saturating_add(1),
        workspace.y.saturating_add(1),
        workspace.width.saturating_sub(2),
        workspace.height.saturating_sub(2),
    );
    // Workspace-level tab footer at the bottom of the inner region.
    let footer_h = crate::common::layout::text::footer_height(
        &crate::common::layout::hints::sql_workspace_footer_text(),
        inner.width,
    );
    Some(Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer_h),
    ))
}

pub(crate) fn iw_body_area_for_hit(
    size: ratatui::layout::Size,
    state: &AppState,
) -> Option<ratatui::layout::Rect> {
    if !state.explorer.instances.active_is_instance()
        || state.modal.is_some()
        || matches!(state.focus, Pane::Discover(_))
    {
        return None;
    }
    let footer_h = global_footer_height(state, size.width);
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    if body_h < 5 {
        return None;
    }
    let workspace = workspace_rect_for_hit(size, body_top, body_h, state)?;
    // IW outer Block (1 col/row border).
    let inner = ratatui::layout::Rect::new(
        workspace.x.saturating_add(1),
        workspace.y.saturating_add(1),
        workspace.width.saturating_sub(2),
        workspace.height.saturating_sub(2),
    );
    if inner.width == 0 || inner.height < 3 {
        return None;
    }
    // Footer height matches instance_workspace/view.rs's wrapped-line-count
    // calculation for IW's pane footer + overview status (overview status only
    // when the active tab is Overview).
    let mut footer_text =
        crate::common::layout::hints::instance_workspace_footer_text(state.iw.pane);
    if matches!(state.iw.pane, crate::app_shell::nav::IwPane::Overview)
        && let Some(status) = state.iw.overview.status.as_deref()
        && !status.is_empty()
    {
        footer_text.push('\n');
        footer_text.push_str(status);
    }
    use crate::common::utils::text_width::wrapped_line_count;
    let footer_h = wrapped_line_count(&footer_text, inner.width)
        .max(1)
        .min(inner.height.saturating_sub(2).max(1));
    // Split: tab bar (1) + body + footer.
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Min(0),
            ratatui::layout::Constraint::Length(footer_h),
        ])
        .split(inner);
    Some(chunks[1])
}

/// Compute the context picker overlay rect (in the active tab's editor) for
/// mouse hit-testing, or `None` when the picker is closed / not shown.
pub(crate) fn sql_picker_area_for_hit(
    size: ratatui::layout::Size,
    state: &AppState,
) -> Option<ratatui::layout::Rect> {
    if state.modal.is_some()
        || matches!(state.focus, Pane::Discover(_))
        || state.instance_workspace_open()
        || state.sql.sql_tab.tabs.is_empty()
    {
        return None;
    }
    let tab = state.sql.sql_tab.active_tab()?;
    if !tab.editor.context_picker.open {
        return None;
    }
    let footer_h = global_footer_height(state, size.width);
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    if body_h < 3 {
        return None;
    }
    let workspace = workspace_rect_for_hit(size, body_top, body_h, state)?;
    // Outer " SQL Workspace " border (1 col/row).
    let inner = Rect::new(
        workspace.x.saturating_add(1),
        workspace.y.saturating_add(1),
        workspace.width.saturating_sub(2),
        workspace.height.saturating_sub(2),
    );
    let footer_h = crate::common::layout::text::footer_height(
        &crate::common::layout::hints::sql_workspace_footer_text(),
        inner.width,
    );
    let sql_tab_area = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer_h),
    );
    if sql_tab_area.height < 1 {
        return None;
    }
    // The SQL tab's body sits below its 1-row tab bar.
    let sql_body = Rect::new(
        sql_tab_area.x,
        sql_tab_area.y.saturating_add(1),
        sql_tab_area.width,
        sql_tab_area.height.saturating_sub(1),
    );
    let layout = crate::features::sql_workspace::sql_tab::layout::sql_tab_layout(
        sql_body,
        tab.splitter.editor_top_height,
        tab.splitter.history_pane_width,
    );
    crate::features::sql_workspace::sql_tab::editor::layout::context_picker_area(
        layout.editor,
        true,
    )
}

/// Geometry needed to float the results picker popups (rows-per-page / page
/// jump) above their toolbar button: the results pane's inner area (the popup
/// clamp / clear region) plus the exact anchor rect of the invoked control.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ResultsPickerAnchor {
    pub pane_inner: Rect,
    pub anchor: Rect,
}

/// Resolve the anchor of a results pagination-toolbar control, mirroring the
/// same layout chain the renderer uses (workspace border → sql tab body →
/// results pane → `layout_pagination_bar`). When the toolbar is not shown (no
/// result rows) the anchor defaults to where the toolbar would sit, so the
/// picker still floats inside the results pane instead of covering the whole
/// workspace.
pub(crate) fn results_picker_anchor(
    state: &AppState,
    workspace: Rect,
    hit: crate::features::sql_workspace::sql_tab::results::pagination::ResultsPaginationHit,
) -> Option<ResultsPickerAnchor> {
    if state.instance_workspace_open() || state.sql.sql_tab.tabs.is_empty() {
        return None;
    }
    let tab = state.sql.sql_tab.active_tab()?;
    if workspace.width <= 2 || workspace.height <= 2 {
        return None;
    }
    // " SQL Workspace " outer border (1 row/col).
    let inner = Rect::new(
        workspace.x.saturating_add(1),
        workspace.y.saturating_add(1),
        workspace.width.saturating_sub(2),
        workspace.height.saturating_sub(2),
    );
    // Workspace-level tab footer at the bottom of the inner region.
    let footer_h = crate::common::layout::text::footer_height(
        &crate::common::layout::hints::sql_workspace_footer_text(),
        inner.width,
    );
    let sql_tab_area = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer_h),
    );
    if sql_tab_area.height <= 2 {
        return None;
    }
    // The SQL tab's body sits below its 1-row tab bar.
    let body = Rect::new(
        sql_tab_area.x,
        sql_tab_area.y.saturating_add(1),
        sql_tab_area.width,
        sql_tab_area.height.saturating_sub(1),
    );
    let layout = crate::features::sql_workspace::sql_tab::layout::sql_tab_layout(
        body,
        tab.splitter.editor_top_height,
        tab.splitter.history_pane_width,
    );
    if layout.results.width <= 2 || layout.results.height <= 2 {
        return None;
    }
    // The results pane's own outer Block border.
    let pane_inner = Rect::new(
        layout.results.x.saturating_add(1),
        layout.results.y.saturating_add(1),
        layout.results.width.saturating_sub(2),
        layout.results.height.saturating_sub(2),
    );
    if pane_inner.width == 0 || pane_inner.height == 0 {
        return None;
    }
    let results_layout =
        crate::features::sql_workspace::sql_tab::results::layout::compute_results_layout(
            pane_inner,
            tab.results.detail_open,
            tab.results.splitter.detail_pane_width,
            tab.results.list.row_count(),
            tab.results.list.search.text_input_active(),
            &tab.results.list.executed_sql_display(),
        );
    let list = &tab.results.list;
    let anchor = results_layout
        .pagination
        .filter(|_| list.row_count() > 0)
        .and_then(|pag| {
            let (counting, show_count) = list.toolbar_count_flags();
            let bar =
                crate::features::sql_workspace::sql_tab::results::pagination::layout_pagination_bar(
                    pag,
                    list.row_limit,
                    list.page,
                    list.total_rows(),
                    list.row_count(),
                    counting,
                    show_count,
                );
            bar.hits
                .into_iter()
                .find(|(h, _)| *h == hit)
                .map(|(_, r)| r)
        })
        // No toolbar (no result rows): anchor where the toolbar would sit so
        // the popup still floats near the controls.
        .unwrap_or_else(|| {
            let x = pane_inner.right().saturating_sub(12).max(pane_inner.x);
            let y = pane_inner.bottom().saturating_sub(2).max(pane_inner.y);
            Rect {
                x,
                y,
                width: pane_inner.right().saturating_sub(x),
                height: 1,
            }
        });
    Some(ResultsPickerAnchor { pane_inner, anchor })
}

/// The results picker popup (rows-per-page / page jump) as drawn: its outer
/// rect plus the per-preset row rects a row-limit picker offers (empty for the
/// page input, which has no selectable rows). Geometry is shared by the
/// renderer and the mouse hit-testing so a click lands exactly where a preset
/// is drawn.
#[derive(Debug, Clone)]
pub(crate) struct ResultsPickerPopup {
    pub popup: Rect,
    /// One rect per `ResultsRowLimitPicker` preset, in list order. A click on
    /// one of these applies that row limit; a click elsewhere inside the popup
    /// (hint row / page input) keeps it open.
    pub preset_rows: Vec<Rect>,
}

/// Resolve the geometry of the open results picker popup (if any), mirroring
/// the size and anchoring `app::view` uses to render it. Returns `None` when no
/// results picker modal is open or the layout cannot be resolved.
pub(crate) fn results_picker_popup(
    state: &AppState,
    workspace: Rect,
) -> Option<ResultsPickerPopup> {
    use crate::app::state::ModalKind;
    let (hit, width, preset_count, height) = match &state.modal {
        Some(ModalKind::ResultsRowLimitPicker { limits, .. }) => {
            // Body = one row per preset (no footer hint row) plus both borders.
            // Keep in lockstep with `render_results_picker_popup`.
            let rows = limits.len();
            (
                crate::features::sql_workspace::sql_tab::results::pagination::ResultsPaginationHit::RowLimit,
                30u16,
                limits.len(),
                2 + rows as u16,
            )
        }
        // Body = "Current: page n of m" + "Go to: [input]" (no footer hint row),
        // plus both borders.
        Some(ModalKind::ResultsPageInput { .. }) => (
            crate::features::sql_workspace::sql_tab::results::pagination::ResultsPaginationHit::PageNumber,
            32u16,
            0,
            4,
        ),
        _ => return None,
    };
    let anchor = results_picker_anchor(state, workspace, hit)?;
    let popup = crate::features::sql_workspace::sql_tab::results::pagination::popup_above_anchor(
        anchor.anchor,
        anchor.pane_inner,
        width,
        height,
    );
    if popup.width < 2 || popup.height < 2 {
        return None;
    }
    // Preset rows run from the first body row (below the top border) down to
    // the hint row; the renderer draws them in the same order.
    let preset_rows = (0..preset_count)
        .map(|i| Rect {
            x: popup.x.saturating_add(1),
            y: popup.y.saturating_add(1).saturating_add(i as u16),
            width: popup.width.saturating_sub(2),
            height: 1,
        })
        .filter(|r| r.bottom() <= popup.bottom().saturating_sub(1))
        .collect();
    Some(ResultsPickerPopup { popup, preset_rows })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{AppState, ModalKind};
    use crate::features::sql_workspace::sql_tab::results::state::QueryResultData;

    /// A SQL workspace with one tab showing a paginated 300-row result and an
    /// open rows-per-page picker, over a wide workspace region.
    fn picker_state() -> (AppState, Rect) {
        let mut state = AppState::default();
        state.sql.sql_tab.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        {
            let tab = &mut state.sql.sql_tab.tabs[0];
            tab.results.list.paginated = true;
            tab.results.list.page = 1;
            tab.results.list.row_limit = 100;
            tab.results.list.result = Some(QueryResultData {
                columns: Vec::new(),
                rows: vec![Vec::new(); 100],
                rows_affected: None,
                total_rows: Some(300),
            });
        }
        state.modal = Some(ModalKind::ResultsRowLimitPicker {
            current: 100,
            limits: vec![50, 100, 500, 1000],
        });
        (state, Rect::new(0, 3, 200, 50))
    }

    #[test]
    fn row_limit_popup_matches_preset_rows() {
        let (state, workspace) = picker_state();
        let popup = results_picker_popup(&state, workspace).expect("popup geometry");
        assert_eq!(
            popup.preset_rows.len(),
            4,
            "one hit row per rows-per-page preset"
        );
        // Rows start under the top border and each is one cell tall.
        assert_eq!(popup.preset_rows[0].y, popup.popup.y + 1);
        assert_eq!(popup.preset_rows[1].y, popup.popup.y + 2);
        // The whole popup stays inside the workspace and above the toolbar.
        assert!(popup.popup.y >= workspace.y);
        assert!(popup.popup.right() <= workspace.right());
    }

    #[test]
    fn no_picker_modal_yields_no_popup_geometry() {
        let (mut state, workspace) = picker_state();
        state.modal = None;
        assert!(results_picker_popup(&state, workspace).is_none());
    }
}
