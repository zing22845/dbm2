//! Local database instance discovery.

mod context;
mod fingerprint;
mod merge;
mod postgresql_conf;
mod providers;
mod scanner;
mod tsv;
mod types;

pub use context::{
    DiscoveryConfig, DiscoveryTarget, is_loopback_host, parse_port_spec, split_host_ports,
    validate_host,
};
pub use fingerprint::compute_fingerprint;
pub use scanner::{DiscoveryScanner, ScanOptions, ScanProgress};
pub use tsv::parse_targets_tsv;
pub use types::{Confidence, DiscoveredInstance, DiscoverySource, InstanceRunStatus, ScanResult};
