//! Effect subsystem: side-effecting operations (IO, async work) that
//! features request from their update functions. Each effect is executed
//! by the `EffectRunner` and produces zero or more `Action`s.

pub mod effect;
pub mod effect_trait;
pub mod runner;

pub use effect::ShellEffect;
pub use effect_trait::{Effect, ErasedEffect};
pub use runner::{EffectHandle, EffectRunner};
