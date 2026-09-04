//! Explorer feature rendering.
//!
//! The explorer is a parent pane that hosts two child sub-panes — the
//! instances tree (top) and the objects tree (bottom) — inside a single
//! bordered " Explorer " block, mirroring the original dbm's layout. Both
//! child panes are always visible; the one that owns focus draws an active
//! border.

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::{ExplorerPane, ExplorerState};
use super::instances::view as instances_view;
use super::objects::view as objects_view;
use super::splitter::view as splitter_view;

/// Render the explorer parent pane: a bordered " Explorer " block wrapping the
/// instances tree (top) and objects tree (bottom). `focused` controls whether
/// the outer explorer border is the active one; the active child sub-pane's own
/// border highlights too (matching the original dbm where the focused pane
/// draws an active border).
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &ExplorerState,
    focused: bool,
    splitter_hover: bool,
    splitter_drag: bool,
) {
    let p = theme.palette();

    // The explorer parent pane wraps both child panes in a single bordered
    // block. Only the outer border reacts to the shell focus so the whole
    // region reads as one pane; the active child sub-pane highlights below.
    let outer = ratatui::widgets::Block::default()
        .title(" Explorer ")
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(p.zone_border(focused));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    if inner.width == 0 || inner.height < 3 {
        return;
    }

    // Instances (top) and objects (bottom) stacked vertically, split by a
    // 1-row draggable splitter, mirroring the original dbm explorer layout.
    // The splitter width is owned by the explorer `splitter` child feature.
    let panes = super::splitter::view::explorer_body_layout(inner, state.splitter.instances_height);

    let instances_focused = focused && state.pane == ExplorerPane::Instances;
    let objects_focused = focused && state.pane == ExplorerPane::Objects;
    instances_view::render(frame, theme, panes.instances, &state.instances, instances_focused);
    splitter_view::render(frame, &panes, splitter_hover, splitter_drag);
    objects_view::render(frame, theme, panes.objects, &state.objects, objects_focused);
}
