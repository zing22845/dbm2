//! Pure-function utilities shared across features.
//!
//! `utils` holds stateless, side-effect-free helpers (display-width math,
//! shortcut label formatting, and in the future hashing / string formatting).
//! Nothing here depends on `app`, on features, or on IO.

pub mod cursor;
pub mod shortcuts;
pub mod sql_editability;
pub mod text_width;
pub mod zone_nav;
