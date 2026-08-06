//! Pure drawing helpers, layout math and style functions.
//!
//! Reusable presentation utilities that are not a full TEA component. Kept
//! separate from `components/` so rendering primitives are independently
//! testable and themeable.

pub mod format;
pub mod overlay_clear;
pub mod pane_scrollbar;
pub mod splitter;
pub mod theme;
