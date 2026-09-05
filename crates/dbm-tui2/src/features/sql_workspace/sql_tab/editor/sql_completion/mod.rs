//! SQL completion sub-module (editor intelligence).
//!
//! Provides keyword / table / column completion for the SQL editor. The core
//! engine (context analysis, semantic model, provider) is pure and
//! editor-agnostic: it depends on the `Cursor` abstraction rather than on a
//! concrete editor crate, so it can be tested independently and reused once the
//! editor is wired in.

pub mod alias;
pub mod alias_blacklist;
pub mod context;
pub mod effect;
pub mod engine;
pub mod ident;
pub mod input;
pub mod intent;
pub mod keywords;
pub mod match_score;
pub mod msg;
pub mod preferred;
pub mod provider;
pub mod semantic;
pub mod state;
pub mod tokens;
pub mod update;
pub mod view;
