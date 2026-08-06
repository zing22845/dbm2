//! The application layer: the central message router, global state,
//! central update dispatcher and the run loop.

pub mod action;
pub mod input;
pub mod loop_mod;
pub mod msg;
pub mod session;
pub mod state;
pub mod update;
pub mod view;

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
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run_async())
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
                .try_init();
        }
        None => {
            let filter =
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(std::io::stderr)
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
