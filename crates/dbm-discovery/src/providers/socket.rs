use std::path::PathBuf;

use dbm_core::Engine;
use walkdir::WalkDir;

use crate::context::DiscoveryConfig;
use crate::providers::DiscoveryProvider;
use crate::types::{Confidence, DiscoveryCandidate, DiscoverySource, InstanceRunStatus};

pub struct SocketProvider;

impl DiscoveryProvider for SocketProvider {
    fn discover(&self, _config: &DiscoveryConfig) -> Vec<DiscoveryCandidate> {
        let mut out = Vec::new();
        for root in socket_roots() {
            if !root.exists() {
                continue;
            }
            for entry in WalkDir::new(&root)
                .max_depth(2)
                .into_iter()
                .filter_map(Result::ok)
            {
                let name = entry.file_name().to_string_lossy();
                if let Some(port) = parse_socket_name(&name) {
                    out.push(DiscoveryCandidate {
                        engine: Engine::Postgres,
                        host: "127.0.0.1".into(),
                        port,
                        socket_path: Some(entry.path().display().to_string()),
                        data_dir: None,
                        systemd_unit: None,
                        version: None,
                        status: InstanceRunStatus::Running,
                        source: DiscoverySource::Socket,
                        confidence: Confidence::Medium,
                    });
                }
            }
        }
        out
    }
}

fn socket_roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/var/run/postgresql"), PathBuf::from("/tmp")]
}

fn parse_socket_name(name: &str) -> Option<u16> {
    name.strip_prefix(".s.PGSQL.")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_socket_port() {
        assert_eq!(parse_socket_name(".s.PGSQL.5432"), Some(5432));
    }
}
