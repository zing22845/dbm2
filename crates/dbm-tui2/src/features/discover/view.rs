//! Discover feature rendering.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};

use crate::common::view::pane_scrollbar::ActiveScrollbar;
use crate::common::view::theme::Theme;

use super::engine::view as engine_view;
use super::layout::{discover_body_area, discover_status};
use super::results::view as results_view;
use super::splitter::view as splitter_view;
use super::state::DiscoverState;
use super::targets::view as targets_view;

/// Render the discover feature: engine selector on top, with the targets editor
/// and results list stacked vertically below (mirrors the original dbm layout,
/// where targets sits above results and they are separated by a splitter row).
/// Each pane's border highlights when it owns focus, and the discover dialog
/// gets a footer (pane nav + scan/close hints) across the bottom.
#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &DiscoverState,
    focus: crate::app_shell::nav::DiscoverPane,
    splitter_hover: bool,
    splitter_drag: bool,
    targets_layout_out: &std::cell::RefCell<Option<targets_view::TargetsLayoutInfo>>,
    results_layout_out: &std::cell::RefCell<Option<usize>>,
    active_scrollbar: Option<ActiveScrollbar>,
) -> Option<crate::common::editor::EditorHardwareCursor> {
    use crate::common::utils::text_width::wrapped_line_count;
    use crate::common::view::hints::{
        discover_engine_footer_text, discover_footer_text, draw_pane_footer,
    };
    let p = theme.palette();

    // The discover parent pane wraps the three child panes (engine, targets,
    // results) inside a single bordered block, matching the original dbm.
    let outer = ratatui::widgets::Block::default()
        .title(" Discover ")
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(p.popup_border(true));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    let footer_text = discover_footer_text(&discover_status(state));
    let footer_h = if footer_text.is_empty() {
        0
    } else {
        footer_text.lines().count().clamp(1, 2) as u16
    };

    // The engine pane is border (2) + a one-row label + its wrapped footer. Its
    // footer can span a hints line plus the "only Postgres" status note, so the
    // pane must grow to fit the wrapped status instead of being a fixed size
    // (otherwise the second footer line is clipped away).
    let engine_footer_text = discover_engine_footer_text(state.engine.status.as_deref());
    let engine_footer_h = if engine_footer_text.is_empty() {
        0
    } else {
        wrapped_line_count(&engine_footer_text, inner.width.saturating_sub(2).max(1))
            .max(1)
            .min(inner.height.saturating_sub(3).max(1))
    };
    let engine_h = 3u16.saturating_add(engine_footer_h);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(engine_h), // engine selector + its footer
            Constraint::Min(0),           // targets + results
            Constraint::Length(footer_h), // discover dialog footer
        ])
        .split(inner);

    engine_view::render(frame, theme, chunks[0], &state.engine, focus);

    // Stacked vertically, like the original: targets (top), a 1-row splitter,
    // then results (bottom). The splitter width is owned by the discover
    // `splitter` child feature (targets height in rows).
    let body =
        super::splitter::layout::discover_body_layout(chunks[1], state.splitter.targets_height);

    let caret = targets_view::render(
        frame,
        theme,
        body.targets,
        &state.targets,
        focus,
        targets_layout_out,
        active_scrollbar,
    );
    splitter_view::render(frame, &body, splitter_hover, splitter_drag);
    results_view::render(
        frame,
        theme,
        body.results,
        &state.results,
        focus,
        results_layout_out,
        active_scrollbar,
    );

    // Discover dialog footer: rendered directly (no separator dashes), so the
    // style matches every other footer.
    if footer_h > 0 {
        draw_pane_footer(frame, theme, chunks[2], &footer_text);
    }

    // The close-confirmation dialog: asking to close the discover modal shows a
    // centered popup. Enter confirms (Close), Esc cancels (CancelClose); the
    // keys are routed in the discover input layer.
    if state.close_confirm {
        render_close_confirm(frame, theme, area);
        return None; // the confirm popup covers the edit cell
    }
    caret
}

/// Render the "close discovery?" confirmation popup over the whole discover
/// area. It reuses the shared confirm popup (title + body + Yes/No buttons) so
/// it looks identical to every other confirm dialog. Pure `state -> view`.
fn render_close_confirm(frame: &mut Frame, theme: &Theme, area: Rect) {
    crate::common::view::modal::render_confirm_popup(
        frame,
        theme,
        area,
        " Close Discover ",
        vec![Line::from(Span::raw(
            "Close Discover and return to the tree?",
        ))],
        true,
    );
}

/// The discover targets/results body height (rows): the track the horizontal
/// splitter is a percentage of. Convenience wrapper over [`discover_body_area`].
pub fn discover_body_track(area: Rect, state: &DiscoverState) -> u16 {
    discover_body_area(area, state).height.max(1)
}
