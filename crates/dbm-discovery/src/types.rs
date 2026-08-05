use dbm_core::Engine;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySource {
    PidFile,
    Process,
    Port,
    Socket,
    Systemd,
    Docker,
}

impl DiscoverySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PidFile => "pidfile",
            Self::Process => "process",
            Self::Port => "port",
            Self::Socket => "socket",
            Self::Systemd => "systemd",
            Self::Docker => "docker",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceRunStatus {
    Running,
    Stopped,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredInstance {
    pub discovery_id: String,
    pub fingerprint: String,
    pub engine: Engine,
    pub host: String,
    pub port: u16,
    pub socket_path: Option<String>,
    pub data_dir: Option<String>,
    pub systemd_unit: Option<String>,
    pub version: Option<String>,
    pub status: InstanceRunStatus,
    pub sources: Vec<DiscoverySource>,
    pub confidence: Confidence,
    pub already_registered: bool,
    pub registered_instance_id: Option<String>,
    pub scanned_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub scan_id: String,
    pub instances: Vec<DiscoveredInstance>,
}

/// Partial candidate before merge / id assignment.
#[derive(Debug, Clone)]
pub struct DiscoveryCandidate {
    pub engine: Engine,
    pub host: String,
    pub port: u16,
    pub socket_path: Option<String>,
    pub data_dir: Option<String>,
    pub systemd_unit: Option<String>,
    pub version: Option<String>,
    pub status: InstanceRunStatus,
    pub source: DiscoverySource,
    pub confidence: Confidence,
}

impl InstanceRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Unknown => "unknown",
        }
    }
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

impl DiscoveryCandidate {
    pub fn fingerprint(&self) -> String {
        crate::fingerprint::compute_fingerprint(
            self.engine,
            self.data_dir.as_deref(),
            &self.host,
            self.port,
            self.socket_path.as_deref(),
            self.systemd_unit.as_deref(),
        )
    }
}
