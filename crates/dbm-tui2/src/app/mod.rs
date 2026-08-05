//! The application layer: the central message router, global state,
//! central update dispatcher and the run loop.

pub mod action;
pub mod loop_mod;
pub mod msg;
pub mod state;
pub mod update;
pub mod view;

pub use msg::AppMsg;
pub use state::{AppState, ModalKind};


/// Binary entry point: run the TUI until the user quits.
pub fn run() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run_async())
}

/// Async entry point used by `run`.
pub async fn run_async() -> anyhow::Result<()> {
    crate::app::loop_mod::run_event_loop().await
}

// Re-export the trait helpers so downstream feature code can route intents
// and effects through the central router without reaching into `app_shell`.
pub use crate::app_shell::effect::ErasedEffect;
pub use crate::app_shell::intent::RoutableIntent;
