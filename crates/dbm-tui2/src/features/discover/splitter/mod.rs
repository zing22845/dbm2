//! Discover child feature: the horizontal splitter between the targets editor
//! and the results list. It owns `targets_height` (the top pane height in
//! rows); the percentage is only materialized for session persistence /
//! resizes.

pub mod layout;
pub mod state;
pub mod view;
