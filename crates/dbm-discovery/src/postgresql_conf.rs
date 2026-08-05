use std::path::Path;

/// Read `port` from `{data_dir}/postgresql.conf` when it is not set on the command line.
pub fn read_port_from_config(data_dir: &str) -> Option<u16> {
    let path = Path::new(data_dir).join("postgresql.conf");
    let content = std::fs::read_to_string(path).ok()?;
    parse_port_from_conf_content(&content)
}

fn parse_port_from_conf_content(content: &str) -> Option<u16> {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.split('#').next()?.trim();
        let rest = line.strip_prefix("port")?;
        let rest = rest.trim_start();
        if !rest.starts_with('=') {
            continue;
        }
        let value = rest[1..].trim().trim_matches('"').trim_matches('\'');
        if let Ok(port) = value.parse::<u16>() {
            return Some(port);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_port_variants() {
        let conf = r#"
#port = 5432
port = 5482
listen_addresses = '*'
"#;
        assert_eq!(parse_port_from_conf_content(conf), Some(5482));

        assert_eq!(parse_port_from_conf_content("port=5433\n"), Some(5433));
        assert_eq!(parse_port_from_conf_content("port = '5440'\n"), Some(5440));
        assert!(parse_port_from_conf_content("# only comments\n").is_none());
    }

    #[test]
    fn read_port_from_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("postgresql.conf"), "port = 5482\n").unwrap();
        assert_eq!(
            read_port_from_config(dir.path().to_str().unwrap()),
            Some(5482)
        );
    }
}
