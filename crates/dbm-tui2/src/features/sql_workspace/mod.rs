//! SQL workspace feature (Feature 7).
//!
//! Per the architecture inventory, `sql_workspace` owns a `sql_tab` parent
//! feature that manages tabs and contains the three child modules
//! `editor`, `results` and `history`. This file is the top of the nested
//! TEA tree; its `update` delegates to `sql_tab::update`.

pub mod effect;
pub mod intent;
pub mod msg;
pub mod sql_tab;
pub mod state;
pub mod update;
pub mod view;
