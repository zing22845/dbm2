use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use dbm_core::Engine;

use crate::context::DiscoveryConfig;
use crate::providers::DiscoveryProvider;
use crate::scanner::ScanProgress;
use crate::types::{Confidence, DiscoveryCandidate, DiscoverySource, InstanceRunStatus};

pub struct PortProvider;

impl PortProvider {
    pub fn discover_with_options(
        &self,
        config: &DiscoveryConfig,
        progress: Option<&dyn Fn(ScanProgress)>,
        cancel: Option<&AtomicBool>,
    ) -> Vec<DiscoveryCandidate> {
        let targets = config.effective_targets();
        let hosts_total = u32::try_from(targets.len()).unwrap_or(u32::MAX);
        let mut out = Vec::new();

        for (host_index, target) in targets.iter().enumerate() {
            if is_cancelled(cancel) {
                break;
            }
            for port in &target.ports {
                if is_cancelled(cancel) {
                    return out;
                }
                if let Some(candidate) = probe_host_port(&target.host, *port) {
                    out.push(candidate);
                }
            }
            // Report progress only after a host's ports have been probed, so
            // `N/N` is reached exactly when scanning finishes — matching the
            // original dbm. A slow final host therefore shows `(N-1)/N` (still
            // scanning) rather than a full bar that looks done.
            if let Some(callback) = progress {
                callback(ScanProgress {
                    hosts_done: u32::try_from(host_index + 1).unwrap_or(u32::MAX),
                    hosts_total,
                });
            }
        }

        out
    }
}

impl DiscoveryProvider for PortProvider {
    fn discover(&self, config: &DiscoveryConfig) -> Vec<DiscoveryCandidate> {
        self.discover_with_options(config, None, None)
    }
}

fn is_cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

fn probe_host_port(host: &str, port: u16) -> Option<DiscoveryCandidate> {
    let addrs: Vec<SocketAddr> = (host, port).to_socket_addrs().ok()?.collect();
    for addr in addrs {
        if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(300)) {
            let confidence = if looks_like_postgres(&stream) {
                Confidence::Medium
            } else {
                continue;
            };
            let host_str = match addr.ip() {
                std::net::IpAddr::V6(v6) if v6.is_loopback() => "127.0.0.1".to_string(),
                ip => ip.to_string(),
            };
            return Some(DiscoveryCandidate {
                engine: Engine::Postgres,
                host: host_str,
                port,
                socket_path: None,
                data_dir: None,
                systemd_unit: None,
                version: None,
                status: InstanceRunStatus::Running,
                source: DiscoverySource::Port,
                confidence,
            });
        }
    }
    None
}

fn looks_like_postgres(mut stream: &TcpStream) -> bool {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));

    // SSLRequest: 8 bytes, magic 80877103
    if stream.write_all(&[0, 0, 0, 8, 4, 210, 22, 47]).is_err() {
        return false;
    }

    let mut buf = [0u8; 1];
    match stream.read(&mut buf) {
        Ok(1) => matches!(buf[0], b'S' | b'N' | b'E'),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_closed_port_returns_none() {
        assert!(probe_host_port("127.0.0.1", 1).is_none());
    }
}
