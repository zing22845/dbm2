use dbm_core::Engine;
use sha2::{Digest, Sha256};

pub fn compute_fingerprint(
    engine: Engine,
    data_dir: Option<&str>,
    host: &str,
    port: u16,
    socket_path: Option<&str>,
    systemd_unit: Option<&str>,
) -> String {
    let material = format!(
        "{}|{}|{}|{}|{}|{}",
        engine,
        data_dir.unwrap_or(""),
        host,
        port,
        socket_path.unwrap_or(""),
        systemd_unit.unwrap_or(""),
    );
    let digest = Sha256::digest(material.as_bytes());
    format!("sha256:{:x}", digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_fingerprint() {
        let a = compute_fingerprint(
            Engine::Postgres,
            Some("/var/lib/postgresql/16/main"),
            "127.0.0.1",
            5432,
            None,
            None,
        );
        let b = compute_fingerprint(
            Engine::Postgres,
            Some("/var/lib/postgresql/16/main"),
            "127.0.0.1",
            5432,
            None,
            None,
        );
        assert_eq!(a, b);
        assert!(a.starts_with("sha256:"));
    }
}
