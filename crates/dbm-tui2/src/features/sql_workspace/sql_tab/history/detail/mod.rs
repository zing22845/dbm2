//! History detail child feature.
//!
//! Full TEA (State → Msg → Update → View) child of `history`. Owns the
//! detail pane scroll and pinned SQL state (`state`), its own message enum
//! (`msg`), a pure by-value `update` transition (plus the cross-feature
//! `reconcile_on_selection_change` helper), and the themed renderer (`view`).

pub mod layout;
pub mod msg;
pub mod state;
pub mod update;
pub mod view;
