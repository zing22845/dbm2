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
use crate::features::global_footer::view as footer_view;

/// The splitter a mouse press resolved to, i.e. the one being drag-resized.
///
/// A single `Option` (rather than one flag per splitter) makes a drag mutually
/// exclusive **by construction**: `Down` arms at most one target, and `Drag`
/// dispatches on that target alone, so a drag can never resize a second split.
///
/// Each variant also fixes the axis its drag reads — a vertical splitter owns a
/// width (reads `x`), a horizontal one owns a height (reads `y`) — so dragging
/// a splitter cannot move the other dimension either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SplitterDrag {
    /// App-level Explorer / workspace splitter (vertical: width, reads `x`).
    App,
    /// Explorer instances / objects splitter (horizontal: height, reads `y`).
    Explorer,
    /// Discover targets / results splitter (horizontal: height, reads `y`).
    Discover,
    /// A SQL-tab splitter inside the given tab (axis depends on the variant).
    Sql(
        usize,
        crate::features::sql_workspace::sql_tab::splitter::view::SqlSplitter,
    ),
}

/// The app body layout (explorer + vertical splitter + workspace), computed once
/// from the same `app_body_layout` the render uses. This is the single geometry
/// source for app-level mouse hit-testing, so every region it derives (explorer,
/// workspace) agrees with the rendered splitter (no hard-coded 20% drift when
/// the Explorer is resized).
pub(crate) fn app_body_geometry(
    size: ratatui::layout::Size,
    body_top: u16,
    body_h: u16,
    state: &AppState,
) -> Option<crate::features::app_splitter::view::AppBodyLayout> {
    let body_area = Rect::new(0, body_top, size.width, body_h);
    let layout = crate::features::app_splitter::view::app_body_layout(
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
    let footer_h = footer_view::footer_height(&state.footer, size.width);
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
    let footer_h = crate::common::view::hints::footer_height(
        &crate::common::view::hints::sql_workspace_footer_text(),
        inner.width,
    );
    Some(Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer_h),
    ))
}

/// Resolve which splitter a press at `(x, y)` starts a drag on.
///
/// Covers the splitters hit-tested **after** the click has re-mapped focus —
/// the app-level Explorer/workspace one, the Explorer instances/objects one and
/// the SQL-tab ones. (The discover splitter is resolved earlier, while focus is
/// still the pre-click one, so it is deliberately not part of this function.)
///
/// Candidates are checked in a fixed priority order and the first hit wins, so
/// a press arms **at most one** splitter: a single drag can never resize two
/// splits, and each target later reads only the axis it owns. Every candidate
/// is gated on actually being rendered, so a drag cannot start on a splitter
/// the user cannot see.
pub(crate) fn resolve_splitter_drag(
    state: &AppState,
    size: ratatui::layout::Size,
    x: u16,
    y: u16,
) -> Option<SplitterDrag> {
    let body_top = 3u16;
    let body_h = size
        .height
        .saturating_sub(body_top)
        .saturating_sub(footer_view::footer_height(&state.footer, size.width));
    if body_h < 3 {
        return None;
    }

    // The app-level Explorer / workspace splitter is draggable from any focus
    // pane: it separates two peer top-level panes rather than the panes of one
    // feature.
    {
        let body_area = Rect::new(0, body_top, size.width, body_h);
        let layout = crate::features::app_splitter::view::app_body_layout(
            body_area,
            state.splitter.explorer_pane_width,
        );
        if crate::features::app_splitter::view::splitter_at(&layout, x, y) {
            return Some(SplitterDrag::App);
        }
    }

    // The Explorer instances / objects splitter: only while the Explorer owns
    // focus, since the split lives inside the Explorer pane.
    if matches!(state.focus, Pane::Explorer(_))
        && let Some(explorer) = app_explorer_rect(size, body_top, body_h, state)
        && explorer.height >= 3
    {
        let inner = Rect::new(
            explorer.x.saturating_add(1),
            explorer.y.saturating_add(1),
            explorer.width.saturating_sub(2),
            explorer.height.saturating_sub(2),
        );
        let layout = crate::features::explorer::splitter::view::explorer_body_layout(
            inner,
            state.explorer.splitter.instances_height,
        );
        if crate::features::explorer::splitter::view::splitter_at(&layout, x, y) {
            return Some(SplitterDrag::Explorer);
        }
    }

    // The SQL-tab splitters: only while the SQL workspace owns focus. The
    // feature resolves the point to a splitter; the shell only supplies the
    // area and the coordinates.
    if state.focus == Pane::SQLWorkspace
        && let Some(tab_area) = sql_tab_area_for_hit(size, state)
        && let Some((tab_id, splitter)) =
            crate::features::sql_workspace::sql_tab::splitter::view::sql_tab_splitter_at(
                &state.sql.sql_tab,
                tab_area,
                x,
                y,
            )
    {
        return Some(SplitterDrag::Sql(tab_id, splitter));
    }

    None
}

/// Compute the Instance Workspace's **body** rect (active sub-pane content,
/// inside the tab bar and parent footer), or `None` when IW is not shown.
/// Mirrors `instance_workspace/view.rs`'s area splitting logic.
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
    let footer_h = footer_view::footer_height(&state.footer, size.width);
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
    let mut footer_text = crate::common::view::hints::instance_workspace_footer_text(state.iw.pane);
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
    let footer_h = footer_view::footer_height(&state.footer, size.width);
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
    let footer_h = crate::common::view::hints::footer_height(
        &crate::common::view::hints::sql_workspace_footer_text(),
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
    crate::features::sql_workspace::sql_tab::editor::view::context_picker_area(layout.editor, true)
}
#[cfg(test)]
mod tests {
    use super::*;

    /// The app body geometry (top row, height) that mouse hit-testing is
    /// computed against, for a given terminal size.
    fn test_body(state: &AppState, size: ratatui::layout::Size) -> (u16, u16) {
        let body_top = 3u16;
        let body_h = size
            .height
            .saturating_sub(body_top)
            .saturating_sub(footer_view::footer_height(&state.footer, size.width));
        (body_top, body_h)
    }

    /// The rect of the Explorer instances/objects horizontal splitter.
    fn test_explorer_splitter_rect(
        state: &AppState,
        size: ratatui::layout::Size,
        body_top: u16,
        body_h: u16,
    ) -> Rect {
        let explorer = app_explorer_rect(size, body_top, body_h, state).expect("explorer rect");
        let inner = Rect::new(
            explorer.x.saturating_add(1),
            explorer.y.saturating_add(1),
            explorer.width.saturating_sub(2),
            explorer.height.saturating_sub(2),
        );
        crate::features::explorer::splitter::view::explorer_body_layout(
            inner,
            state.explorer.splitter.instances_height,
        )
        .splitter
    }

    /// One press arms **at most one** splitter. Regression guard: the drag
    /// targets used to be four independent booleans that were never reset on
    /// `Up`, so a drag of one splitter also resized every split dragged earlier
    /// (a vertical move changing a height, a horizontal move changing a width).
    #[test]
    fn resolve_splitter_drag_arms_exactly_one_target() {
        use crate::features::explorer::state::ExplorerPane;

        let mut state = AppState::default();
        state.term_width = 120;
        state.term_height = 40;
        // The Explorer owns focus, so its instances/objects splitter is
        // eligible — the app-level splitter must still win on its own column.
        state.focus = Pane::Explorer(ExplorerPane::Instances);
        let size = ratatui::layout::Size::new(120, 40);
        let (body_top, body_h) = test_body(&state, size);
        let layout = crate::features::app_splitter::view::app_body_layout(
            Rect::new(0, body_top, size.width, body_h),
            state.splitter.explorer_pane_width,
        );

        // On the app-level vertical splitter: only the app splitter arms.
        assert_eq!(
            resolve_splitter_drag(&state, size, layout.v_splitter.x, layout.v_splitter.y + 5),
            Some(SplitterDrag::App)
        );

        // On the Explorer's horizontal splitter: only that one arms.
        let ex = test_explorer_splitter_rect(&state, size, body_top, body_h);
        assert_eq!(
            resolve_splitter_drag(&state, size, ex.x + 3, ex.y),
            Some(SplitterDrag::Explorer)
        );

        // Inside a pane (on no splitter at all): nothing arms.
        assert_eq!(resolve_splitter_drag(&state, size, 2, body_top + 1), None);
    }

    /// Every row of the app-level vertical splitter resolves to `App` —
    /// including the row the Explorer's horizontal splitter occupies. The two
    /// own different axes, so a vertical drag must never reach a height split.
    #[test]
    fn vertical_splitter_rows_never_resolve_to_a_height_split() {
        use crate::features::explorer::state::ExplorerPane;

        let mut state = AppState::default();
        state.term_width = 120;
        state.term_height = 40;
        state.focus = Pane::Explorer(ExplorerPane::Instances);
        let size = ratatui::layout::Size::new(120, 40);
        let (body_top, body_h) = test_body(&state, size);
        let layout = crate::features::app_splitter::view::app_body_layout(
            Rect::new(0, body_top, size.width, body_h),
            state.splitter.explorer_pane_width,
        );
        for dy in 0..layout.v_splitter.height {
            assert_eq!(
                resolve_splitter_drag(&state, size, layout.v_splitter.x, layout.v_splitter.y + dy),
                Some(SplitterDrag::App),
                "row {dy} of the vertical splitter must not resolve to another split"
            );
        }
    }

    /// With the SQL workspace focused, a press on the editor/history splitter
    /// arms that splitter alone — not the app-level one — so a drag there cannot
    /// resize the Explorer width (the reported "horizontal drag moves the
    /// Explorer" symptom).
    #[test]
    fn sql_tab_splitter_arms_without_the_app_splitter() {
        use crate::features::sql_workspace::sql_tab::state::SqlFocus;

        let mut state = AppState::default();
        state.term_width = 120;
        state.term_height = 40;
        state.focus = Pane::SQLWorkspace;
        state.sql.sql_tab.open_connection_tab(
            "inst".into(),
            "c1".into(),
            "id1".into(),
            None,
            None,
            None,
        );
        state.sql.sql_tab.tabs[0].focus = SqlFocus::Editor;
        let size = ratatui::layout::Size::new(120, 40);
        let tab_area = sql_tab_area_for_hit(size, &state).expect("sql tab area");
        let body = Rect::new(
            tab_area.x,
            tab_area.y.saturating_add(1),
            tab_area.width,
            tab_area.height.saturating_sub(1),
        );
        let tab = &state.sql.sql_tab.tabs[0];
        let layout = crate::features::sql_workspace::sql_tab::layout::sql_tab_layout(
            body,
            tab.splitter.editor_top_height,
            tab.splitter.history_pane_width,
        );

        let got = resolve_splitter_drag(&state, size, layout.v_splitter.x, layout.v_splitter.y + 1);
        assert!(
            matches!(got, Some(SplitterDrag::Sql(_, _))),
            "a press on the SQL editor/history splitter must arm it, got {got:?}"
        );
        assert_ne!(
            got,
            Some(SplitterDrag::App),
            "the app-level width splitter must not arm alongside it"
        );
    }
}
