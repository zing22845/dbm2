//! Header feature state.

/// The number of header buttons currently rendered.
pub const HEADER_BUTTONS: usize = 1;

/// State for the header feature.
///
/// The header renders the app title bar plus a row of action buttons
/// (`Discover` today). `button` is the cursor index of the currently focused
/// button; it is clamped to `[0, HEADER_BUTTONS)`.
#[derive(Debug, Default, Clone)]
pub struct HeaderState {
    /// Index of the currently focused header button.
    pub button: usize,
    /// The clickable rect of the `Discover` button, recorded on the last render
    /// so mouse hit-testing uses exactly what was drawn (mirrors the original
    /// `header_button_rects` in `ui_layout`).
    pub discover_button_rect: Option<ratatui::layout::Rect>,
}
