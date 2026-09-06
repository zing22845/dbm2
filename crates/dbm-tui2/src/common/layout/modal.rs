//! Popup geometry shared by the renderer, the mouse hit-testers and the
//! update layer.

use ratatui::layout::Rect;

/// The centered `width_pct` × `height_pct` popup rect over `base` (pure
/// geometry). Shared by the renderer and the mouse hit-tester so both agree on
/// where a popup sits.
pub fn popup_rect(base: Rect, width_pct: u16, height_pct: u16) -> Rect {
    let w = (base.width * width_pct) / 100;
    let h = (base.height * height_pct) / 100;
    Rect {
        x: base.x + (base.width - w) / 2,
        y: base.y + (base.height - h) / 2,
        width: w,
        height: h,
    }
}

/// The centered rect of a confirm popup over `area` (pure geometry). The height
/// grows with the body so multi-line content (e.g. an unregister message) fits
/// without overlapping the Yes/No button row. Shared by the renderer and the
/// mouse hit-tester so both agree on the popup position.
pub fn confirm_popup_rect(area: Rect, body_rows: usize) -> Rect {
    let popup_w = area.width.clamp(28, 44);
    // border top + body rows + blank row + button row + border bottom.
    let popup_h = (body_rows as u16).saturating_add(4).clamp(6, 14);
    Rect {
        x: area
            .x
            .saturating_add(area.width.saturating_sub(popup_w) / 2),
        y: area
            .y
            .saturating_add(area.height.saturating_sub(popup_h) / 2),
        width: popup_w,
        height: popup_h.min(area.height),
    }
}
