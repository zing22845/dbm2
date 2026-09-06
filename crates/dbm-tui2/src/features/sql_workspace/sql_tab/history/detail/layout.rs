//! History detail geometry: the text column width.

/// Text area width inside the Detail block (borders excluded).
pub fn detail_text_width(pane_width: u16) -> u16 {
    pane_width.saturating_sub(2).max(1)
}
