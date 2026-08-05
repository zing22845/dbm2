//! Local SQLite store for DBM instance registry and connection credentials.

mod audit;
mod crypto;
mod discovery;
mod error;
mod instance;
mod instance_connection;
mod kv_store;
mod lifecycle;
mod migrate;
mod paths;
mod register_precheck;
mod resolve;
mod sql_history;
mod store;
mod tui_session;
mod version;

pub use discovery::RunDiscoveryOptions;
pub use lifecycle::{LifecycleStatus, probe_lifecycle_status};
pub use version::parse_version_short;
pub use error::{StoreError, StoreResult};
pub use instance::ManagedInstance;
pub use instance_connection::{
    ConnectionPrecheck, InstanceConnection, NewInstanceConnection, UpdateInstanceConnection,
    format_connection_precheck,
};
pub use paths::{data_dir, db_path, init_data_dir, master_key_path};
pub use register_precheck::{
    InstancePrecheck, PrecheckIssue, PrecheckLevel, RegisterOptions, RegisterResult,
    format_precheck_report,
};
pub use resolve::{SessionResolveOptions, SqlSession, resolve_database_url, resolve_sql_session};
pub use kv_store::{kv_load_tui_session, kv_save_tui_session};
pub use sql_history::SQL_HISTORY_MAX_PER_CONNECTION;
pub use store::Store;
pub use tui_session::{
    TUI_SESSION_VERSION, TuiInstanceWorkspaceSnapshot, TuiSessionSnapshot, TuiTabSnapshot,
    TuiTreeSelection, TuiTreeSnapshot, load_tui_session, save_tui_session,
};

pub use dbm_discovery::{DiscoveredInstance, DiscoveryConfig, ScanResult, parse_port_spec};
