use dbm_core::Engine;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedInstance {
    pub id: String,
    pub fingerprint: String,
    pub name: String,
    pub engine: Engine,
    pub host: String,
    pub port: u16,
    pub socket_path: Option<String>,
    pub data_dir: Option<String>,
    pub env_label: Option<String>,
    pub registered_at: String,
    pub version_full: Option<String>,
    pub version_short: Option<String>,
    pub version_checked_at: Option<String>,
    pub lifecycle_status: Option<String>,
    pub lifecycle_checked_at: Option<String>,
    pub lifecycle_detail: Option<String>,
}

impl ManagedInstance {
    pub fn display_target(&self) -> String {
        format!("{}@{}:{}/postgres", self.engine, self.host, self.port)
    }
}
