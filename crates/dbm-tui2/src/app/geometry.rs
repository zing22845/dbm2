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
    let footer_h = footer_layout::footer_height(&state.footer, size.width);
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
    let footer_h = footer_layout::footer_height(&state.footer, size.width);
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
    let mut footer_text = crate::common::layout::hints::instance_workspace_footer_text(state.iw.pane);
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
    let footer_h = footer_layout::footer_height(&state.footer, size.width);
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
