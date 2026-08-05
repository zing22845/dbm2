/// Extract short version from `SELECT version()` first line.
pub fn parse_version_short(full: &str) -> String {
    let line = full.lines().next().unwrap_or(full).trim();
    if let Some(rest) = line.strip_prefix("PostgreSQL ") {
        let token = rest
            .split(|c: char| c.is_whitespace() || c == ',')
            .next()
            .unwrap_or("");
        if !token.is_empty()
            && token.chars().next().is_some_and(|c| c.is_ascii_digit())
            && token.chars().all(|c| c.is_ascii_digit() || c == '.')
        {
            return token.to_string();
        }
    }
    let mut out: String = line.chars().take(64).collect();
    if line.chars().count() > 64 {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_postgres_version_line() {
        let full = "PostgreSQL 16.2 on x86_64-apple-darwin, compiled by Apple clang";
        assert_eq!(parse_version_short(full), "16.2");
    }

    #[test]
    fn parses_patch_level() {
        let full = "PostgreSQL 15.4.1 on x86_64-pc-linux-gnu";
        assert_eq!(parse_version_short(full), "15.4.1");
    }

    #[test]
    fn fallback_truncates_unknown_shape() {
        let full = "NotAPostgresBanner ".repeat(10);
        let short = parse_version_short(&full);
        assert!(!short.is_empty());
        assert!(short.chars().count() <= 65);
        assert!(short.ends_with('…'));
    }
}
