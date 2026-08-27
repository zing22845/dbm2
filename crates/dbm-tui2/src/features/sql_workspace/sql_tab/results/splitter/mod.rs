//! Results-internal detail/list splitter.
//!
//! When `detail_open`, the Results Block's inner content splits horizontally
//! into `[list | splitter | detail]`. The splitter sub-feature owns the
//! `detail_pane_width` and provides geometry helpers for layout, rendering,
//! and hit-testing.

pub mod state;
pub mod view;
