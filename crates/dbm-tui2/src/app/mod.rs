//! The application layer: the central message router, global state,
//! central update dispatcher and the run loop.
//!
//! The children are grouped by their role in the TEA data flow
//! (`input -> msg -> update -> view`):
//!
//! - **core**: [`msg`] (the message enumeration), [`state`] (the model),
//!   [`update`] (the only state transition) and [`view`] (pure rendering);
//! - **input adapters**: [`key`] and [`mouse`] turn terminal events into an
//!   [`AppMsg`]. They only *read* state and never mutate it, so they are not
//!   part of `update` — and `update` must never depend on them (or on
//!   [`geometry`], since hit-testing reads the layout the renderer draws);
//! - **runtime**: [`loop_mod`] (event loop), [`round`] (draining the message
//!   cascade) and [`action`] (results of running effects);
//! - **shared**: [`confirm`] (modal -> action, used by both input channels),
//!   [`geometry`] (layout source shared by hit-testing and rendering) and
//!   [`session`] (state <-> on-disk snapshot).

// --- TEA core ---
pub mod msg;
pub mod state;
pub mod update;
pub mod view;

// --- Input adapters: terminal events -> AppMsg (state is read-only) ---
pub mod key;
pub mod mouse;

// --- Runtime: event loop, message cascade, effects ---
pub mod action;
pub mod loop_mod;
pub mod round;

// --- Shared / cross-cutting ---
pub mod confirm;
pub mod geometry;
pub mod session;

pub use msg::AppMsg;
pub use state::{AppState, ModalKind};

/// Binary entry point: run the TUI until the user quits.
///
/// Debug logging is off by default (the default `warn`/stderr subscriber is
/// installed). Use [`run_with_log_file`] to capture `debug` logs to a file.
pub fn run() -> anyhow::Result<()> {
    run_with_log_file(None)
}

/// Run the TUI, optionally writing `debug`-level logs to `log_file`.
///
/// - `Some(path)` writes `debug` logs to that file (created/truncated). Useful
///   for tracing interaction bugs without corrupting the TUI screen.
/// - `None` installs the default subscriber (level from `RUST_LOG`, default
///   `warn`, written to stderr).
pub fn run_with_log_file(log_file: Option<std::path::PathBuf>) -> anyhow::Result<()> {
    init_tracing(log_file);
    install_panic_hook();
    let runtime = tokio::runtime::Runtime::new()?;
    // Catch a panic from anywhere in the TUI (event loop, update, render) so a
    // crash is reported as an error carrying the panic message, instead of
    // unwinding out of `main`. The hook installed above has already restored the
    // terminal and printed the message plus a backtrace, so this only shapes the
    // process exit; it never swallows the diagnostic.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(run_async())
    })) {
        Ok(result) => result,
        Err(payload) => {
            let message = panic_payload_message(payload);
            Err(anyhow::anyhow!("TUI panicked: {message}"))
        }
    }
}

/// Install a panic hook that restores the terminal **before** the panic report
/// is written, so a crash never leaves the terminal in raw mode / the alternate
/// screen — it would look frozen and swallow the panic text. Mirrors the
/// original dbm's `install_panic_hook`.
///
/// The hook also forces a backtrace: the default hook only prints one when
/// `RUST_BACKTRACE` is set, so without this a crash reported from the field
/// would have no stack to go on unless the user reproduced it with the env var.
fn install_panic_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            crate::app::loop_mod::restore_terminal();
            default_hook(info);
            eprintln!("{}", std::backtrace::Backtrace::force_capture());
        }));
    });
}

/// Extract the human-readable message from a caught panic payload.
fn panic_payload_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        return (*msg).to_string();
    }
    if let Some(msg) = payload.downcast_ref::<String>() {
        return msg.clone();
    }
    "unknown panic".to_string()
}

/// Initialize the tracing subscriber.
///
/// With a log file, `debug` spans/events are appended (truncating) to that file
/// so they stay fully separate from the TUI's stdout screen. Without one, the
/// default `warn`/stderr subscriber is used and `RUST_LOG` still applies.
pub fn init_tracing(log_file: Option<std::path::PathBuf>) {
    use tracing_subscriber::EnvFilter;
    match log_file {
        Some(path) => {
            let dir = path.parent().unwrap_or(std::path::Path::new("."));
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "dbm-debug.log".to_string());
            let file_appender = tracing_appender::rolling::never(dir, file_name);
            let (writer, guard) = tracing_appender::non_blocking(file_appender);
            // Keep the non-blocking guard alive for the process lifetime so log
            // records are not dropped at exit.
            std::mem::forget(guard);
            let _ = tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::new("debug"))
                .with_writer(writer)
                // Include the source file and line number of each log call so
                // issues can be traced back to the exact code location.
                .with_file(true)
                .with_line_number(true)
                .try_init();
        }
        None => {
            let filter =
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(std::io::stderr)
                .with_file(true)
                .with_line_number(true)
                .try_init();
        }
    }
}

/// Async entry point used by `run`.
pub async fn run_async() -> anyhow::Result<()> {
    crate::app::loop_mod::run_event_loop().await
}

// Re-export the trait helpers so downstream feature code can route intents
// and effects through the central router without reaching into `app_shell`.
pub use crate::app_shell::effect::ErasedEffect;
pub use crate::app_shell::intent::RoutableIntent;
