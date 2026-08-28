//! History list child feature.
//!
//! Full TEA (State → Msg → Update → View) child of `history`. Owns the list
//! cursor, horizontal/vertical scroll, and `/` search state (in `state`),
//! its own message enum (`msg`), a pure by-value `update` transition, and
//! a renderer + hit-test helpers (`view`).

pub mod msg;
pub mod state;
pub mod update;
pub mod view;
