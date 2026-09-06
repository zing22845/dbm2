//! Geometry shared by the renderer, the mouse hit-testers and the update
//! layer: pure "where is X on screen / how far may it move" calculations.
//!
//! These live outside [`crate::common::view`] on purpose — view depends on
//! layout, never the other way round, so update / input / state never have to
//! reach into the rendering layer to compute a rect or a delta.

pub mod hints;
pub mod modal;
pub mod splitter;
pub mod text;
