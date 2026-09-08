//! Popup geometry shared by the renderer, the mouse hit-testers and the
//! update layer.

use ratatui::layout::Rect;

use crate::common::utils::text_width::{char_width, width as display_width};

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

/// Wrap a single statement into visual rows that each fit `cols` display cells
/// (CJK-aware). Newlines inside the statement start a new row.
pub fn wrap_statement_rows(text: &str, cols: u16) -> Vec<String> {
    let cols = (cols as usize).max(1);
    let mut out = Vec::new();
    for raw in text.split('\n') {
        if raw.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut row = String::new();
        let mut used = 0usize;
        for ch in raw.chars() {
            let cw = char_width(ch).max(1);
            if used > 0 && used + cw > cols {
                out.push(std::mem::take(&mut row));
                used = 0;
            }
            row.push(ch);
            used += cw;
        }
        out.push(row);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// The full resolved geometry of the commit-preview popup: a bordered, centered
/// popup whose width grows with the longest statement and whose height grows
/// with the wrapped content — each clamped to what fits the workspace. When the
/// SQL needs more rows than the popup's maximum, the body becomes vertically
/// scrollable (`visible_rows < total_rows`).
///
/// `rows` holds each statement's wrapped visual rows (before the line-number
/// gutter), matching what the renderer paints, so the same data sizes the
/// popup and drives the scrollbar.
pub struct CommitPreviewLayout {
    pub rect: Rect,
    /// Line-number gutter width (digits of the statement count + pad).
    pub gutter_w: u16,
    /// The width in display cells available for SQL text (after gutter).
    pub body_cols: u16,
    /// Wrapped visual rows per statement, in statement order.
    pub rows: Vec<Vec<String>>,
    /// Total wrapped visual rows across all statements.
    pub total_rows: usize,
    /// Rows the popup body can show without scrolling.
    pub visible_rows: usize,
}

impl CommitPreviewLayout {
    pub fn max_scroll(&self) -> usize {
        self.total_rows.saturating_sub(self.visible_rows)
    }
}

/// Compute the commit-preview popup over `area`. Returns `None` when there is
/// no room for even a minimal popup.
pub fn commit_preview_layout(area: Rect, statements: &[String]) -> Option<CommitPreviewLayout> {
    if area.width < 20 || area.height < 6 || statements.is_empty() {
        return None;
    }
    let n = statements.len();
    let gutter_w = ((n.max(1).to_string().len() + 1) as u16).max(2);
    // The popup never exceeds the discover modal's footprint (75% x 75% of the
    // workspace), so the commit preview can never be bigger than discover.
    let cap_w = area.width.saturating_mul(75) / 100;
    let cap_h = area.height.saturating_mul(75) / 100;
    if cap_w < 20 || cap_h < 6 {
        return None;
    }
    // Borders (2) + pad column (1) eat into the popup width.
    let max_cols = cap_w.saturating_sub(gutter_w.saturating_add(3)).max(1);
    // Prefer the longest natural statement width, clamped into [min, max].
    let natural = statements
        .iter()
        .flat_map(|s| s.split('\n'))
        .map(display_width)
        .max()
        .unwrap_or(0)
        .max(1) as u16;
    let body_cols = natural.max(24).min(max_cols);
    let rows: Vec<Vec<String>> = statements
        .iter()
        .map(|s| wrap_statement_rows(s, body_cols))
        .collect();
    let total_rows = rows.iter().map(|r| r.len()).sum::<usize>().max(1);

    // Auto height up to the discover cap; beyond that the body scrolls.
    let max_body = (cap_h.saturating_sub(4) as usize).max(1);
    let visible_rows = total_rows.min(max_body);

    // popup width = borders + gutter + text + pad.
    let mut popup_w = gutter_w.saturating_add(body_cols).saturating_add(3);
    if popup_w > cap_w {
        popup_w = cap_w;
    }
    let mut popup_h = (visible_rows as u16).saturating_add(4);
    if popup_h > cap_h {
        popup_h = cap_h;
    }
    Some(CommitPreviewLayout {
        rect: Rect {
            x: area
                .x
                .saturating_add(area.width.saturating_sub(popup_w) / 2),
            y: area
                .y
                .saturating_add(area.height.saturating_sub(popup_h) / 2),
            width: popup_w,
            height: popup_h,
        },
        gutter_w,
        body_cols,
        rows,
        total_rows,
        visible_rows,
    })
}

/// The popup rect of the commit preview (pure geometry), shared by rendering
/// and mouse hit-testing so the Yes/No buttons always sit where they are drawn.
pub fn commit_preview_rect(area: Rect, statements: &[String]) -> Option<Rect> {
    commit_preview_layout(area, statements).map(|l| l.rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_rows_break_wide_and_embedded_newlines() {
        let rows = wrap_statement_rows("ab cd", 3);
        // 5 cells at width 3 wrap to 2 rows.
        assert_eq!(rows, vec!["ab ".to_string(), "cd".to_string()]);
        let rows = wrap_statement_rows("one\ntwo", 100);
        assert_eq!(rows, vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn commit_preview_auto_sizes_and_scrolls_oversized_sql() {
        // A few short statements fit without scrolling and size to content.
        let short = vec!["select 1".to_string(), "select 2".to_string()];
        let area = Rect::new(0, 0, 120, 40);
        let l = commit_preview_layout(area, &short).expect("roomy area");
        assert_eq!(l.total_rows, 2);
        assert_eq!(l.visible_rows, 2);
        assert_eq!(l.max_scroll(), 0);
        assert!(l.rect.height < area.height, "small content: small popup");

        // Enough SQL to overflow the discover-height cap becomes scrollable.
        let big: Vec<String> = (0..40)
            .map(|i| format!("UPDATE users SET note = 'row {i} quite long content that wraps'"))
            .collect();
        let l2 = commit_preview_layout(area, &big).expect("roomy area");
        assert!(
            l2.total_rows > l2.visible_rows,
            "oversized SQL must overflow the body (total {} > visible {})",
            l2.total_rows,
            l2.visible_rows
        );
        assert!(l2.max_scroll() > 0, "oversized SQL needs a scroll range");
        assert!(l2.rect.width <= area.width);
        assert!(l2.rect.height <= area.height);
        assert!(l2.rect.height >= 6);
        // Never bigger than the discover modal (75% x 75% of the workspace).
        assert!(l2.rect.width <= area.width * 3 / 4);
        assert!(l2.rect.height <= area.height * 3 / 4);
    }
}
