use dbm_core::Engine;
use regex::Regex;

#[cfg(target_os = "linux")]
use std::fs;

use crate::context::DiscoveryConfig;
use crate::postgresql_conf::read_port_from_config;
use crate::providers::DiscoveryProvider;
use crate::types::{Confidence, DiscoveryCandidate, DiscoverySource, InstanceRunStatus};

pub struct ProcessProvider;

impl DiscoveryProvider for ProcessProvider {
    fn discover(&self, _config: &DiscoveryConfig) -> Vec<DiscoveryCandidate> {
        #[cfg(target_os = "linux")]
        {
            discover_proc()
        }
        #[cfg(not(target_os = "linux"))]
        {
            discover_ps()
        }
    }
}

#[cfg(target_os = "linux")]
fn discover_proc() -> Vec<DiscoveryCandidate> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return out;
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let Some(pid) = entry.file_name().to_string_lossy().parse::<u32>().ok() else {
            continue;
        };
        let cmdline_path = entry.path().join("cmdline");
        let Ok(raw) = fs::read(&cmdline_path) else {
            continue;
        };
        if raw.is_empty() {
            continue;
        }
        let cmdline = raw
            .split(|b| *b == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect::<Vec<_>>();
        if let Some(candidate) = parse_postgres_cmdline(&cmdline) {
            let _ = pid;
            out.push(candidate);
        }
    }
    out
}

#[cfg(not(target_os = "linux"))]
fn discover_ps() -> Vec<DiscoveryCandidate> {
    use std::process::Command;

    let output = Command::new("ps").args(["-ax", "-o", "command="]).output();
    let Ok(output) = output else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter_map(|line| parse_postgres_command_line(line.trim()))
        .collect()
}

#[cfg(target_os = "linux")]
fn parse_postgres_cmdline(parts: &[String]) -> Option<DiscoveryCandidate> {
    if parts.is_empty() {
        return None;
    }
    let joined = parts.join(" ");
    parse_postgres_command_line(&joined)
}

fn parse_postgres_command_line(line: &str) -> Option<DiscoveryCandidate> {
    if !line.contains("postgres") && !line.contains("postmaster") {
        return None;
    }
    if line.contains("postgres:") {
        // worker process
        return None;
    }

    let data_re = Regex::new(r"(?:-D|--pgdata=)\s*(\S+)").ok()?;
    let port_re = Regex::new(r"(?:-p|--port=)\s*(\d+)").ok()?;

    let data_dir = data_re
        .captures(line)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string());
    let cmdline_port = port_re
        .captures(line)
        .and_then(|cap| cap.get(1))
        .and_then(|m| m.as_str().parse().ok());
    let port = resolve_port(data_dir.as_deref(), cmdline_port);

    Some(DiscoveryCandidate {
        engine: Engine::Postgres,
        host: "127.0.0.1".into(),
        port,
        socket_path: None,
        data_dir,
        systemd_unit: None,
        version: None,
        status: InstanceRunStatus::Running,
        source: DiscoverySource::Process,
        confidence: Confidence::High,
    })
}

fn resolve_port(data_dir: Option<&str>, cmdline_port: Option<u16>) -> u16 {
    if let Some(port) = cmdline_port {
        return port;
    }
    if let Some(dir) = data_dir
        && let Some(port) = read_port_from_config(dir)
    {
        return port;
    }
    5432
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_postgres_process_line() {
        let line = "/usr/lib/postgresql/16/bin/postgres -D /var/lib/postgresql/16/main -p 5432";
        let candidate = parse_postgres_command_line(line).unwrap();
        assert_eq!(candidate.port, 5432);
        assert_eq!(
            candidate.data_dir.as_deref(),
            Some("/var/lib/postgresql/16/main")
        );
    }

    #[test]
    fn port_from_postgresql_conf_when_not_on_cmdline() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("postgresql.conf"), "port = 5482\n").unwrap();
        let data_dir = dir.path().to_string_lossy();
        let line = format!("/opt/homebrew/bin/postgres -D {data_dir}");
        let candidate = parse_postgres_command_line(&line).unwrap();
        assert_eq!(candidate.port, 5482);
        assert_eq!(candidate.data_dir.as_deref(), Some(data_dir.as_ref()));
    }

    #[test]
    fn cmdline_port_overrides_postgresql_conf() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("postgresql.conf"), "port = 5482\n").unwrap();
        let data_dir = dir.path().to_string_lossy();
        let line = format!("/opt/homebrew/bin/postgres -D {data_dir} -p 5433");
        let candidate = parse_postgres_command_line(&line).unwrap();
        assert_eq!(candidate.port, 5433);
    }
}
