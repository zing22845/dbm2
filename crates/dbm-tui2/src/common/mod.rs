//! Cross-cutting helpers shared by multiple features.
//!
//! `common` holds feature-agnostic code, organized by technical responsibility
//! so it does not degrade into a catch-all:
//!
//! - `model/`       : shared domain types (ids, errors, config structs).
//! - `view/`        : pure drawing helpers, layout math, style functions.
//! - `controller/`  : controller abstractions (traits, state machines, debounce).
//! - `service/`     : external interaction abstractions (db pools, file IO, net).
//! - `components/`  : reusable TEA UI components (list, input, tabs).
//! - `utils/`       : pure-function utilities (formatting, hashing, string ops).
//!
//! `common` deliberately depends on neither `app` nor any feature. Empty layers
//! are stubbed with a `mod.rs` documenting their intended responsibility; add
//! modules there as content migrates in.

pub mod components;
pub mod controller;
pub mod editor;
pub mod layout;
pub mod model;
pub mod service;
pub mod utils;
pub mod view;
