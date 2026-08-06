//! Shared service abstractions and the composition-root service bundle.
//!
//! `Services` aggregates every infrastructure dependency (database store, and
//! later file IO, network clients). It is created once at the composition root
//! (`app`) and injected into the `EffectRunner`, which hands a clone to each
//! effect as it runs. Features never construct or hold services directly —
//! effects describe *what* to do, and `Services` supplies the *how*.

pub mod clipboard;
pub mod services;
