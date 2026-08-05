use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    init_tracing();
    dbm_tui2::app::run()
}

/// Initialise the `tracing` subscriber so `tracing::*` macros actually emit
/// output. The filter level is read from the `RUST_LOG` environment variable
/// (e.g. `RUST_LOG=warn`); defaulting to `warn` so operational warnings surface
/// without being noisy.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
