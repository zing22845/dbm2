fn main() -> anyhow::Result<()> {
    // `app::run()` installs the tracing subscriber (default warn/stderr) and
    // drives the event loop. Debug logging to a file is available via the
    // `dbm interact --debug-log <path>` CLI option.
    dbm_tui2::app::run()
}
