//! Explorer child feature: the horizontal splitter between the instances tree
//! and the objects tree. It owns `instances_height` (the top pane height in
//! rows); the percentage is only materialized for session persistence /
//! resizes.

pub mod state;
pub mod view;
