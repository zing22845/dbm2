use std::net::IpAddr;
use std::time::Duration;

use dbm_core::Engine;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryTarget {
    pub host: String,
    pub ports: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    pub targets: Vec<DiscoveryTarget>,
    pub hosts: Vec<String>,
    pub ports: Vec<u16>,
    pub max_duration: Duration,
    pub engine: Engine,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            targets: vec![],
            hosts: vec!["127.0.0.1".into()],
            ports: parse_port_spec("5432,5433-5440").expect("default port spec is valid"),
            max_duration: Duration::from_secs(30),
            engine: Engine::Postgres,
        }
    }
}

impl DiscoveryConfig {
    pub fn effective_targets(&self) -> Vec<DiscoveryTarget> {
        if !self.targets.is_empty() {
            return self.targets.clone();
        }

        self.hosts
            .iter()
            .map(|host| DiscoveryTarget {
                host: host.clone(),
                ports: self.ports.clone(),
            })
            .collect()
    }

    pub fn with_extra_hosts(mut self, hosts: impl IntoIterator<Item = String>) -> Self {
        if !self.targets.is_empty() {
            let ports = if self.ports.is_empty() {
                parse_port_spec("5432,5433-5440").expect("default port spec is valid")
            } else {
                self.ports.clone()
            };
            for host in hosts {
                if !self.targets.iter().any(|target| target.host == host) {
                    self.targets.push(DiscoveryTarget {
                        host,
                        ports: ports.clone(),
                    });
                }
            }
            return self;
        }

        for host in hosts {
            if !self.hosts.iter().any(|h| h == &host) {
                self.hosts.push(host);
            }
        }
        self
    }
}

pub fn parse_port_spec(spec: &str) -> Result<Vec<u16>, String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("port spec is empty".into());
    }

    let mut ports = Vec::new();
    let mut saw_part = false;

    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        saw_part = true;

        if let Some((start, end)) = part.split_once('-') {
            let start = start
                .trim()
                .parse::<u16>()
                .map_err(|_| format!("invalid port in range: {part}"))?;
            let end = end
                .trim()
                .parse::<u16>()
                .map_err(|_| format!("invalid port in range: {part}"))?;
            if start == 0 || end == 0 {
                return Err("port 0 is not allowed".into());
            }
            if start > end {
                return Err(format!("inverted port range: {part}"));
            }
            ports.extend(start..=end);
        } else {
            let port = part
                .parse::<u16>()
                .map_err(|_| format!("invalid port: {part}"))?;
            if port == 0 {
                return Err("port 0 is not allowed".into());
            }
            ports.push(port);
        }
    }

    if !saw_part {
        return Err("port spec has no ports".into());
    }

    ports.sort_unstable();
    ports.dedup();
    Ok(ports)
}

pub fn validate_host(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("host is empty".into());
    }
    if trimmed.contains(char::is_whitespace) {
        return Err("host must not contain whitespace".into());
    }

    let host = if trimmed.starts_with('[') && trimmed.ends_with(']') {
        let inner = &trimmed[1..trimmed.len() - 1];
        match inner.parse::<IpAddr>() {
            Ok(IpAddr::V6(_)) => inner,
            _ => return Err(format!("invalid host: {raw}")),
        }
    } else if trimmed.starts_with('[') || trimmed.ends_with(']') {
        return Err(format!("invalid host: {raw}"));
    } else {
        trimmed
    };

    if host.parse::<IpAddr>().is_ok() {
        return Ok(host.to_string());
    }

    if is_valid_domain(host) {
        return Ok(host.to_string());
    }

    Err(format!("invalid host: {raw}"))
}

/// Split `HOST:PORTS` (or `[IPv6]:PORTS`). Host/ports are not validated here.
pub fn split_host_ports(raw: &str) -> Result<(&str, &str), String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("expected HOST:PORTS".into());
    }

    if trimmed.starts_with('[') {
        let Some(close) = trimmed.find(']') else {
            return Err(format!("invalid target `{raw}`: expected [IPv6]:PORTS"));
        };
        let host = &trimmed[..=close];
        let rest = &trimmed[close + 1..];
        let Some(ports) = rest.strip_prefix(':') else {
            return Err(format!("invalid target `{raw}`: expected [IPv6]:PORTS"));
        };
        if ports.is_empty() {
            return Err(format!("invalid target `{raw}`: expected [IPv6]:PORTS"));
        }
        return Ok((host, ports));
    }

    let Some((host, ports)) = trimmed.rsplit_once(':') else {
        return Err(format!("invalid target `{raw}`: expected HOST:PORTS"));
    };
    if host.is_empty() || ports.is_empty() {
        return Err(format!("invalid target `{raw}`: expected HOST:PORTS"));
    }
    if host.contains(':') {
        return Err(format!(
            "invalid target `{raw}`: IPv6 host must be bracketed as [addr]:PORTS"
        ));
    }
    Ok((host, ports))
}

fn is_valid_domain(host: &str) -> bool {
    if host.is_empty() || host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    host.split('.').all(is_valid_domain_label)
}

fn is_valid_domain_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !label.starts_with('-')
        && !label.ends_with('-')
}

pub fn is_loopback_host(host: &str) -> bool {
    let Ok(normalized) = validate_host(host) else {
        return false;
    };
    if normalized.eq_ignore_ascii_case("localhost") {
        return true;
    }
    normalized
        .parse::<IpAddr>()
        .is_ok_and(|addr| addr.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ports_trims_and_ranges() {
        assert_eq!(
            parse_port_spec(" 5432, 5433-5435 ").unwrap(),
            vec![5432, 5433, 5434, 5435]
        );
    }

    #[test]
    fn parse_ports_rejects_inverted_range() {
        assert!(parse_port_spec("5440-5433").is_err());
    }

    #[test]
    fn parse_ports_rejects_empty() {
        assert!(parse_port_spec("").is_err());
        assert!(parse_port_spec("  ,  ").is_err());
    }

    #[test]
    fn parse_ports_rejects_port_zero() {
        assert!(parse_port_spec("0").is_err());
        assert!(parse_port_spec("0-5432").is_err());
        assert!(parse_port_spec("5432-0").is_err());
    }

    #[test]
    fn validate_host_accepts_ipv4_ipv6_domain() {
        assert_eq!(validate_host(" 127.0.0.1 ").unwrap(), "127.0.0.1");
        assert_eq!(validate_host("[::1]").unwrap(), "::1");
        assert_eq!(validate_host("db.example.com").unwrap(), "db.example.com");
    }

    #[test]
    fn validate_host_rejects_blank_and_spaces() {
        assert!(validate_host("").is_err());
        assert!(validate_host("bad host").is_err());
    }

    #[test]
    fn validate_host_rejects_bracketed_non_ipv6() {
        assert!(validate_host("[db.example.com]").is_err());
        assert!(validate_host("[127.0.0.1]").is_err());
        assert_eq!(validate_host("[::1]").unwrap(), "::1");
    }

    #[test]
    fn split_host_ports_accepts_ipv4_domain_and_bracketed_ipv6() {
        assert_eq!(
            split_host_ports("db.example.com:5432,5433-5440").unwrap(),
            ("db.example.com", "5432,5433-5440")
        );
        assert_eq!(
            split_host_ports("192.168.1.10:5432").unwrap(),
            ("192.168.1.10", "5432")
        );
        assert_eq!(split_host_ports("[::1]:5440").unwrap(), ("[::1]", "5440"));
    }

    #[test]
    fn split_host_ports_rejects_unbracketed_ipv6() {
        let err = split_host_ports("::1:5432").unwrap_err();
        assert!(err.contains("bracketed"), "{err}");
    }

    #[test]
    fn loopback_detection() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("LocalHost"));
        assert!(!is_loopback_host("192.168.1.10"));
        assert!(!is_loopback_host("db.example.com"));
    }

    #[test]
    fn effective_targets_prefers_explicit_targets() {
        let config = DiscoveryConfig {
            targets: vec![DiscoveryTarget {
                host: "a".into(),
                ports: vec![1],
            }],
            hosts: vec!["b".into()],
            ports: vec![2],
            max_duration: Duration::from_secs(1),
            engine: dbm_core::Engine::Postgres,
        };

        assert_eq!(
            config.effective_targets(),
            vec![DiscoveryTarget {
                host: "a".into(),
                ports: vec![1],
            }]
        );
    }

    #[test]
    fn effective_targets_expands_flat_hosts_ports() {
        let config = DiscoveryConfig {
            targets: vec![],
            hosts: vec!["h1".into(), "h2".into()],
            ports: vec![5432],
            max_duration: Duration::from_secs(1),
            engine: dbm_core::Engine::Postgres,
        };

        assert_eq!(
            config.effective_targets(),
            vec![
                DiscoveryTarget {
                    host: "h1".into(),
                    ports: vec![5432],
                },
                DiscoveryTarget {
                    host: "h2".into(),
                    ports: vec![5432],
                },
            ]
        );
    }

    #[test]
    fn extra_hosts_extend_flat_hosts() {
        let config = DiscoveryConfig {
            targets: vec![],
            hosts: vec!["h1".into()],
            ports: vec![5432],
            max_duration: Duration::from_secs(1),
            engine: dbm_core::Engine::Postgres,
        }
        .with_extra_hosts(["h2".into(), "h1".into()]);

        assert_eq!(config.hosts, vec!["h1", "h2"]);
        assert!(config.targets.is_empty());
    }

    #[test]
    fn extra_hosts_extend_explicit_targets_with_shared_ports() {
        let config = DiscoveryConfig {
            targets: vec![DiscoveryTarget {
                host: "h1".into(),
                ports: vec![5433],
            }],
            hosts: vec![],
            ports: vec![5432],
            max_duration: Duration::from_secs(1),
            engine: dbm_core::Engine::Postgres,
        }
        .with_extra_hosts(["h2".into(), "h1".into()]);

        assert_eq!(
            config.targets,
            vec![
                DiscoveryTarget {
                    host: "h1".into(),
                    ports: vec![5433],
                },
                DiscoveryTarget {
                    host: "h2".into(),
                    ports: vec![5432],
                },
            ]
        );
    }

    #[test]
    fn extra_hosts_use_default_ports_when_shared_ports_are_empty() {
        let config = DiscoveryConfig {
            targets: vec![DiscoveryTarget {
                host: "h1".into(),
                ports: vec![5433],
            }],
            hosts: vec![],
            ports: vec![],
            max_duration: Duration::from_secs(1),
            engine: dbm_core::Engine::Postgres,
        }
        .with_extra_hosts(["h2".into()]);

        assert_eq!(
            config.targets[1].ports,
            parse_port_spec("5432,5433-5440").unwrap()
        );
    }

    #[test]
    fn default_config_uses_postgres_flat_targets() {
        let config = DiscoveryConfig::default();

        assert!(config.targets.is_empty());
        assert_eq!(config.hosts, vec!["127.0.0.1"]);
        assert_eq!(config.engine, dbm_core::Engine::Postgres);
    }
}
