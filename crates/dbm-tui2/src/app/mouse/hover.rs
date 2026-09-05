//! Splitter hover highlight and per-frame track normalisation.
//!
//! Hover is derived state: it is recomputed from the live layout whenever the
//! pointer moves (or a drag ends), so the highlight can never disagree with the
//! splitter that is actually drawn under the cursor.

use ratatui::layout::Rect;

use crate::app_shell::pane::Pane;
use crate::features::global_footer::view as footer_view;

use crate::app::geometry::{
    app_body_geometry, app_explorer_rect, sql_tab_area_for_hit, workspace_rect_for_hit,
};

/// Refresh each horizontal splitter's last-laid-out track so keyboard `+`/`-`
/// nudges clamp against the live body height. The tracks are approximated from
/// the terminal size (header/footer/border/tab-bar rows); the exact layout
/// re-clamps at render time anyway, and a small approximation error only shifts
/// where a nudge stops — never the rendered split.
pub(crate) fn normalize_splitter_tracks(
    state: &mut crate::app::state::AppState,
    size: ratatui::layout::Size,
) {
    let footer_h = footer_view::footer_height(&state.footer, size.width);
    // Body height: header (3) at the top, footer at the bottom.
    let body_h = size.height.saturating_sub(3).saturating_sub(footer_h);
    // SQL tab body track: workspace inner, minus the workspace footer and the
    // tab bar. Computed with the same footer logic the workspace render uses.
    let workspace = workspace_rect_for_hit(size, 3, body_h, state);
    // Editor+history track width (the workspace inner width), matching the
    // render (live Explorer splitter width), so `[`/`]` history nudges clamp
    // against the exact editor min width.
    let sql_track_w = workspace.map(|ws| ws.width.saturating_sub(2)).unwrap_or(
        size.width
            .saturating_sub(size.width.saturating_mul(2) / 10)
            .saturating_sub(2),
    );
    let sql_body_h = workspace
        .map(|ws| {
            let inner_w = ws.width.saturating_sub(2);
            let ws_footer_h = crate::common::view::hints::footer_height(
                &crate::common::view::hints::sql_workspace_footer_text(),
                inner_w.max(1),
            );
            ws.height
                .saturating_sub(2)
                .saturating_sub(ws_footer_h)
                .saturating_sub(1)
        })
        .unwrap_or(body_h.saturating_sub(4));
    // Derive the SQL splitter bounds from the layout itself (the same function
    // the render uses), so nudge/drag clamp to the exact rendered boundary.
    for tab in &mut state.sql.sql_tab.tabs {
        let sql_area = Rect::new(0, 0, sql_track_w.max(1), sql_body_h.max(1));
        let layout = crate::features::sql_workspace::sql_tab::layout::sql_tab_layout(
            sql_area,
            tab.splitter.editor_top_height,
            tab.splitter.history_pane_width,
        );
        tab.splitter.editor_top_min = layout.editor_top_min;
        tab.splitter.editor_top_max = layout.editor_top_max.max(layout.editor_top_min);
        tab.splitter.history_min = layout.history_min;
        tab.splitter.history_max = layout.history_max.max(layout.history_min);
    }
    // Explorer instances/objects bounds, derived from the same layout the render
    // uses so nudge clamps to the exact rendered boundary.
    if let Some(explorer) = app_explorer_rect(size, 3, body_h, state) {
        let inner = Rect::new(
            explorer.x + 1,
            explorer.y + 1,
            explorer.width.saturating_sub(2),
            explorer.height.saturating_sub(2),
        );
        let layout = crate::features::explorer::splitter::view::explorer_body_layout(
            inner,
            state.explorer.splitter.instances_height,
        );
        state.explorer.splitter.instances_min = layout.instances_min;
        state.explorer.splitter.instances_max = layout.instances_max.max(layout.instances_min);
    }
    // Discover targets/results bounds, from the same popup/body the render lays.
    if let Some(ws) = workspace {
        let discover_popup = crate::common::view::modal::popup_rect(ws, 75, 75);
        let body =
            crate::features::discover::view::discover_body_area(discover_popup, &state.discover);
        let layout = crate::features::discover::splitter::view::discover_body_layout(
            body,
            state.discover.splitter.targets_height,
        );
        state.discover.splitter.targets_min = layout.targets_min;
        state.discover.splitter.targets_max = layout.targets_max.max(layout.targets_min);
    }
}

/// Update splitter hover highlight state from a mouse position.
///
/// Hit-tests every visible splitter against `(x, y)` using the same geometry
/// the renderers employ, so hover highlights exactly the same line that is
/// drawn. Returns `true` when any hover bit actually changed (used to decide
/// whether to request a redraw).
pub(crate) fn update_splitter_hover(
    state: &mut crate::app::state::AppState,
    x: u16,
    y: u16,
    size: ratatui::layout::Size,
) -> bool {
    use crate::common::view::splitter::hit;

    let before = state.splitter_hover;

    // No hover highlight while a modal or the discover close-confirm is active.
    let can_hover = state.modal.is_none() && !state.discover.close_confirm;

    // Preserve the active drag flags — they are managed by the Down/Drag/Up
    // event handlers and must survive a hover recompute (e.g. a `Moved` event
    // arriving mid-drag). Only the hover bits are recomputed here.
    let drag = before.dragging_flags();
    let results_col_resize_drag = before.results_col_resize_drag;
    state.splitter_hover = crate::app::state::SplitterHoverState::default();
    state.splitter_hover.set_dragging_flags(drag);
    state.splitter_hover.results_col_resize_drag = results_col_resize_drag;

    if !can_hover {
        return before != state.splitter_hover;
    }

    let footer_h = footer_view::footer_height(&state.footer, size.width);
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_h);
    if body_h < 3 {
        return before != state.splitter_hover;
    }

    // --- App-level Explorer / workspace vertical splitter ---
    if let Some(layout) = app_body_geometry(size, body_top, body_h, state) {
        state.splitter_hover.app_splitter = hit(layout.v_splitter, x, y);
    }

    // --- Explorer instances / objects horizontal splitter ---
    if matches!(state.focus, Pane::Explorer(_))
        && let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
    {
        let inner = Rect::new(
            explorer.x.saturating_add(1),
            explorer.y.saturating_add(1),
            explorer.width.saturating_sub(2),
            explorer.height.saturating_sub(2),
        );
        if inner.height >= 3 {
            let layout = crate::features::explorer::splitter::view::explorer_body_layout(
                inner,
                state.explorer.splitter.instances_height,
            );
            state.splitter_hover.explorer_splitter = hit(layout.splitter, x, y);
        }
    }

    // --- Discover targets / results horizontal splitter ---
    if matches!(state.focus, Pane::Discover(_))
        && let Some(workspace) = workspace_rect_for_hit(size, body_top, body_h, state)
    {
        let discover_popup = crate::common::view::modal::popup_rect(workspace, 75, 75);
        let body =
            crate::features::discover::view::discover_body_area(discover_popup, &state.discover);
        if body.height >= 3 {
            let layout = crate::features::discover::splitter::view::discover_body_layout(
                body,
                state.discover.splitter.targets_height,
            );
            state.splitter_hover.discover_splitter = hit(layout.splitter, x, y);
        }
    }

    // --- SQL tab splitters ---
    if state.focus == Pane::SQLWorkspace
        && let Some(tab_area) = sql_tab_area_for_hit(size, state)
        && let Some((_tab_id, splitter)) =
            crate::features::sql_workspace::sql_tab::splitter::view::sql_tab_splitter_at(
                &state.sql.sql_tab,
                tab_area,
                x,
                y,
            )
    {
        // Hit-test resolved — mark the corresponding hover bit.
        match splitter {
            crate::features::sql_workspace::sql_tab::splitter::view::SqlSplitter::EditorResults => {
                state.splitter_hover.sql_editor_results = true;
            }
            crate::features::sql_workspace::sql_tab::splitter::view::SqlSplitter::EditorHistory => {
                state.splitter_hover.sql_editor_history = true;
            }
            crate::features::sql_workspace::sql_tab::splitter::view::SqlSplitter::HistoryDetail => {
                state.splitter_hover.history_detail = true;
            }
            crate::features::sql_workspace::sql_tab::splitter::view::SqlSplitter::ResultsDetail => {
                state.splitter_hover.results_detail = true;
            }
        }
    }

    // Results column-width resize hover: a pointer over a result header
    // splitter highlights that column's border so the resizable region is
    // visible. Uses the same shared list geometry as the render.
    if state.focus == Pane::SQLWorkspace
        && let Some(tab_area) = sql_tab_area_for_hit(size, state)
        && let Some(active_tab) = state.sql.sql_tab.active_tab()
        && let Some(list_inner) =
            crate::features::sql_workspace::sql_tab::view::results_list_rect(active_tab, tab_area)
    {
        state.splitter_hover.results_col_resize_hover =
            crate::features::sql_workspace::sql_tab::results::list::view::col_resize_hit_at(
                list_inner,
                &active_tab.results.list,
                x,
                y,
            );
    }

    before != state.splitter_hover
}
