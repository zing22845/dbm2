//! app_shell: the framework-agnostic shell that owns the event loop,
//! intent routing and effect execution. It does not contain any business
//! logic; features plug into it through the `Intent` and `Effect` traits.
//!
//! # Layering contract (open/closed principle)
//!
//! Treat `app_shell` as a read-only "standard library". It defines the
//! *pluggable abstraction slots*: what an `Intent` is, what an `Effect` is,
//! what a `FocusZone` is, and how messages/actions flow. Business features are
//! the *plugs* that fit into these slots.
//!
//! - **Business feature iteration (the common case):** adding a new feature
//!   (a panel, a tab, a modal) only touches `app/` (aggregation + dispatch)
//!   and `features/` (implementation). `app_shell` must NOT change. This works
//!   because `Intent`/`Effect` are object-safe and their erasure layers
//!   (`RoutableIntent<M>`, `ErasedEffect<A>`) let the central router hold
//!   heterogeneous intents/effects without knowing their concrete types.
//! - **System capability upgrade (rare):** only modify `app_shell` when adding
//!   a system-level capability that no single feature owns, e.g. global hotkeys
//!   (`ShellMsg`/`ShellAction`), a new focus region (`FocusZone`), or a change
//!   to effect execution policy (`EffectRunner`). Keep such changes generic so
//!   existing business features keep compiling unchanged.

pub mod action;
pub mod effect;
pub mod intent;
pub mod msg;
pub mod pane;
