//! Discover popup geometry and the status text its height depends on.

use crate::features::discover::state::DiscoverState;
use ratatui::layout::Rect;

/// A one-line status for the discover dialog footer (scanning with live
/// progress / cancelling / cancelled / last error / register result). Mirrors
/// the original dbm's `Scanning… hosts d/t · c: cancel` status line.
pub(crate) fn discover_status(state: &DiscoverState) -> String {
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
    let body_h = inner
        .height
        .saturating_sub(engine_h)
        .saturating_sub(footer_h);
    Rect::new(inner.x, body_top, inner.width, body_h)
}
