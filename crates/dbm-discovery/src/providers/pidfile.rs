use std::fs;
use std::path::{Path, PathBuf};

use dbm_core::Engine;
use walkdir::WalkDir;

use crate::context::DiscoveryConfig;
use crate::providers::DiscoveryProvider;
use crate::types::{Confidence, DiscoveryCandidate, DiscoverySource, InstanceRunStatus};

pub struct PidFileProvider;

impl DiscoveryProvider for PidFileProvider {
    fn discover(&self, _config: &DiscoveryConfig) -> Vec<DiscoveryCandidate> {
        let mut out = Vec::new();
        for root in default_roots() {
            if !root.exists() {
                continue;
            }
            for entry in WalkDir::new(&root)
                .max_depth(4)
                .into_iter()
                .filter_map(Result::ok)
            {
                if entry.file_name() == "postmaster.pid"
                    && let Some(candidate) = parse_pidfile(entry.path())
                {
                    out.push(candidate);
                }
            }
        }
        out
    }
}

fn default_roots() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/var/lib/postgresql"),
        PathBuf::from("/usr/local/var/postgresql"),
        PathBuf::from("/opt/homebrew/var/postgresql"),
        PathBuf::from("/usr/local/var/postgres"),
    ]
}

fn parse_pidfile(path: &Path) -> Option<DiscoveryCandidate> {
    let content = fs::read_to_string(path).ok()?;
    let mut lines = content.lines();
    let _pid = lines.next()?;
    let data_dir = lines.next()?.to_string();
    let _start = lines.next()?;
    let port = lines
        .next()
        .and_then(|line| line.parse::<u16>().ok())
        .unwrap_or(5432);
    let socket_dir = lines.next().map(str::to_string);

    let socket_path = socket_dir.map(|dir| format!("{dir}/.s.PGSQL.{port}"));

    Some(DiscoveryCandidate {
        engine: Engine::Postgres,
        host: "127.0.0.1".into(),
        port,
        socket_path,
        data_dir: Some(data_dir),
        systemd_unit: None,
        version: None,
        status: InstanceRunStatus::Running,
        source: DiscoverySource::PidFile,
        confidence: Confidence::High,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sample_pidfile() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("postmaster.pid");
        fs::write(
            &pidfile,
            "12345\n/var/lib/postgresql/16/main\n1234567890\n5433\n/var/run/postgresql\n",
        )
        .unwrap();
        let candidate = parse_pidfile(&pidfile).unwrap();
        assert_eq!(candidate.port, 5433);
        assert_eq!(
            candidate.data_dir.as_deref(),
            Some("/var/lib/postgresql/16/main")
        );
    }
}
