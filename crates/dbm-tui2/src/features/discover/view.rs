//! Discover feature rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::common::view::theme::Theme;

use super::state::DiscoverState;
use super::engine::view as engine_view;
use super::results::view as results_view;
use super::targets::view as targets_view;

/// Render the discover feature: engine selector on top, with the targets editor
/// and results list stacked vertically below (mirrors the original dbm layout,
/// where targets sits above results and they are separated by a splitter row).
/// Each pane's border highlights when it owns focus, and the discover dialog
/// gets a footer (pane nav + scan/close hints) across the bottom.
pub fn render(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    state: &DiscoverState,
    focus: crate::app_shell::nav::DiscoverPane,
) -> Option<crate::common::editor::EditorHardwareCursor> {
    use crate::common::utils::text_width::wrapped_line_count;
    use crate::common::view::hints::{discover_engine_footer_text, discover_footer_text, draw_pane_footer};
    let p = theme.palette();

    // The discover parent pane wraps the three child panes (engine, targets,
    // results) inside a single bordered block, matching the original dbm.
    let outer = ratatui::widgets::Block::default()
        .title(" Discover ")
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(p.border_active));
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
            Constraint::Length(engine_h),    // engine selector + its footer
            Constraint::Min(0),              // targets + results
            Constraint::Length(footer_h),    // discover dialog footer
        ])
        .split(inner);

    engine_view::render(frame, theme, chunks[0], &state.engine, focus);

    // Stacked vertically, like the original: targets (top), a 1-row splitter,
    // then results (bottom). The splitter width is owned by the discover
    // `splitter` child feature (targets height in rows).
    let body = super::splitter::view::discover_body_layout(chunks[1], state.splitter.targets_height);

    let caret = targets_view::render(frame, theme, body.targets, &state.targets, focus);
    super::splitter::view::render(frame, &body, false, false);
    results_view::render(frame, theme, body.results, &state.results, focus);

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

/// A one-line status for the discover dialog footer (scanning with live
/// progress / cancelling / cancelled / last error / register result). Mirrors
/// the original dbm's `Scanning… hosts d/t · c: cancel` status line.
fn discover_status(state: &DiscoverState) -> String {
    if state.scanning {
        if state.cancelling {
            "cancelling…".to_string()
        } else if let Some((done, total)) = state.scan_progress {
            format!("Scanning… hosts {done}/{total} · c: cancel")
        } else {
            "Scanning…".to_string()
        }
    } else if state.scan_cancelled {
        "cancelled".to_string()
    } else if state.last_error.is_some() {
        "scan failed".to_string()
    } else if let Some(msg) = &state.register_message {
        msg.clone()
    } else {
        String::new()
    }
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
        vec![Line::from(Span::raw("Close Discover and return to the tree?"))],
        true,
    );
}

/// The discover targets/results body rect (rows) for a given discover `area`,
/// mirroring the engine/footer height split inside the outer border — exactly
/// the `chunks[1]` region the render lays the targets/results splitter into.
/// Both the render and the run loop (drag hit-testing, `+`/`-` track) use this,
/// so the splitter you drag is the splitter you see.
pub fn discover_body_area(area: Rect, state: &DiscoverState) -> Rect {
    use crate::common::utils::text_width::wrapped_line_count;
    use crate::common::view::hints::{discover_engine_footer_text, discover_footer_text};
    let inner = ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .inner(area);
    if inner.width == 0 || inner.height == 0 {
        return inner;
    }
    let footer_text = discover_footer_text(&discover_status(state));
    let footer_h = if footer_text.is_empty() {
        0
    } else {
        footer_text.lines().count().clamp(1, 2) as u16
    };
    let engine_footer_text = discover_engine_footer_text(state.engine.status.as_deref());
    let engine_footer_h = if engine_footer_text.is_empty() {
        0
    } else {
        wrapped_line_count(&engine_footer_text, inner.width.saturating_sub(2).max(1))
            .max(1)
            .min(inner.height.saturating_sub(3).max(1))
    };
    let engine_h = 3u16.saturating_add(engine_footer_h);
    let body_top = inner.y + engine_h;
    let body_h = inner.height.saturating_sub(engine_h).saturating_sub(footer_h);
    Rect::new(inner.x, body_top, inner.width, body_h)
}

/// The discover targets/results body height (rows): the track the horizontal
/// splitter is a percentage of. Convenience wrapper over [`discover_body_area`].
pub fn discover_body_track(area: Rect, state: &DiscoverState) -> u16 {
    discover_body_area(area, state).height.max(1)
}
