//! Feature modules. Each feature is a self-contained TEA component with its
//! own `msg` / `state` / `update` / `view` / `intent` / `effect` modules and
//! plugs into the central router via the `AppMsg` / `Action` enums.

pub mod app_splitter;
pub mod discover;
pub mod explorer;
pub mod global_footer;
pub mod header;
pub mod instance_workspace;
pub mod perf_monitor;
pub mod sql_workspace;
