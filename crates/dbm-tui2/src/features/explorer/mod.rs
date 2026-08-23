//! Explorer feature (Feature 5): the connection tree navigator.
//!
//! Owns two child sub-modules: the `instances` list and the `objects` tree.

pub mod effect;
pub mod instances;
pub mod intent;
pub mod msg;
pub mod objects;
pub mod splitter;
pub mod state;
pub mod update;
pub mod view;
