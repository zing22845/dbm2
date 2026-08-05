//! Lifecycle readiness probe for managed instances (data_dir handle + permissions).

use std::fs::OpenOptions;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleStatus {
    None,
    Ready,
    Degraded,
}

impl LifecycleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Ready => "ready",
            Self::Degraded => "degraded",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "ready" => Some(Self::Ready),
            "degraded" => Some(Self::Degraded),
            _ => None,
        }
    }
}

/// Probe lifecycle readiness from `data_dir` (same rules as register `check_data_dir`).
pub fn probe_lifecycle_status(data_dir: Option<&str>) -> (LifecycleStatus, Option<&'static str>) {
    let Some(raw) = data_dir.map(str::trim).filter(|s| !s.is_empty()) else {
        return (LifecycleStatus::None, Some("NO_DATA_DIR"));
    };
    let path = Path::new(raw);
    if !path.exists() {
        return (LifecycleStatus::Degraded, Some("DATA_DIR_MISSING"));
    }
    if path.is_dir() && std::fs::read_dir(path).is_err() {
        return (LifecycleStatus::Degraded, Some("DATA_DIR_NOT_READABLE"));
    }
    if path.is_dir() && !dir_writable(path) {
        return (LifecycleStatus::Degraded, Some("DATA_DIR_NOT_WRITABLE"));
    }
    (LifecycleStatus::Ready, None)
}

fn dir_writable(path: &Path) -> bool {
    let probe = path.join(".dbm_write_probe");
    match OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn no_data_dir_is_none() {
        assert_eq!(
            probe_lifecycle_status(None),
            (LifecycleStatus::None, Some("NO_DATA_DIR"))
        );
        assert_eq!(
            probe_lifecycle_status(Some("   ")),
            (LifecycleStatus::None, Some("NO_DATA_DIR"))
        );
    }

    #[test]
    fn missing_path_is_degraded() {
        let (status, detail) = probe_lifecycle_status(Some("/no/such/dbm_lifecycle_dir"));
        assert_eq!(status, LifecycleStatus::Degraded);
        assert_eq!(detail, Some("DATA_DIR_MISSING"));
    }

    #[test]
    fn writable_dir_is_ready() {
        let tmp = TempDir::new().unwrap();
        let (status, detail) = probe_lifecycle_status(Some(tmp.path().to_str().unwrap()));
        assert_eq!(status, LifecycleStatus::Ready);
        assert_eq!(detail, None);
        assert!(!tmp.path().join(".dbm_write_probe").exists());
    }

    #[test]
    fn as_str_roundtrip() {
        assert_eq!(LifecycleStatus::Ready.as_str(), "ready");
        assert_eq!(
            LifecycleStatus::parse("degraded"),
            Some(LifecycleStatus::Degraded)
        );
    }
}
