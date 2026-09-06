//! App-level splitter feature: the vertical splitter between the Explorer
//! pane and the workspace region.
//!
//! Unlike the `sql_tab` splitters (which separate panes owned by a single
//! tab), this splitter separates two *peer top-level features* — the explorer
//! and the workspace. There is no common feature parent, so it is an app-level
//! feature itself, owning `explorer_pane_width` (the left side).

pub mod layout;
pub mod state;
pub mod view;
