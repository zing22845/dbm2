//! TLS support for PostgreSQL connections.
//!
//! Every pool is built with a rustls connector; whether TLS is actually used is
//! decided by the connection's `sslmode`, mapped onto
//! [`tokio_postgres::config::SslMode`] — where `Prefer` transparently falls back
//! to a plaintext session when the server does not speak TLS.

use std::sync::Arc;

use dbm_core::{DbmError, Result};
use tokio_postgres::config::SslMode;
use tokio_postgres_rustls::MakeRustlsConnect;

/// Extract `sslmode` from a connection URL and map it to tokio-postgres' mode.
///
/// The parameter is stripped from the returned URL because tokio-postgres
/// rejects values outside libpq's `disable`/`prefer`/`require` set (notably
/// `verify-full`), which we accept and translate ourselves.
pub fn split_ssl_mode(raw_url: &str) -> Result<(String, SslMode)> {
    let mut url = url::Url::parse(raw_url).map_err(|e| DbmError::InvalidUrl(e.to_string()))?;

    let mut mode = SslMode::Prefer;
    let mut keep: Vec<(String, String)> = Vec::new();
    for (key, value) in url.query_pairs() {
        if key.eq_ignore_ascii_case("sslmode") {
            mode = parse_ssl_mode(&value)?;
        } else {
            keep.push((key.into_owned(), value.into_owned()));
        }
    }

    if url.query().is_some() {
        if keep.is_empty() {
            url.set_query(None);
        } else {
            let mut pairs = url.query_pairs_mut();
            pairs.clear();
            for (key, value) in &keep {
                pairs.append_pair(key, value);
            }
        }
    }

    Ok((url.to_string(), mode))
}

/// Map a libpq `sslmode` value onto tokio-postgres' three-state mode.
pub fn parse_ssl_mode(raw: &str) -> Result<SslMode> {
    let normalized = raw.trim().to_ascii_lowercase();
    Ok(match normalized.as_str() {
        "disable" => SslMode::Disable,
        // `allow` has no tokio-postgres equivalent; treat it as `prefer`.
        "allow" | "prefer" | "" => SslMode::Prefer,
        // rustls always verifies the certificate chain, so the
        // `require`/`verify-*` variants collapse into the same mode.
        "require" | "verify-ca" | "verify-full" => SslMode::Require,
        other => {
            return Err(DbmError::InvalidUrl(format!(
                "unsupported sslmode `{other}` (expected disable, allow, prefer, require, verify-ca or verify-full)"
            )));
        }
    })
}

/// Build the rustls connector shared by all PostgreSQL pools.
///
/// Root certificates come from the Mozilla set bundled through `webpki-roots`,
/// so static binaries do not depend on system certificate stores.
pub fn connector() -> MakeRustlsConnect {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring always supports the default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();

    MakeRustlsConnect::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_sslmode_defaults_to_prefer() {
        assert_eq!(parse_ssl_mode("").unwrap(), SslMode::Prefer);
        let (url, mode) = split_ssl_mode("postgres://u@h:5432/db").unwrap();
        assert_eq!(url, "postgres://u@h:5432/db");
        assert_eq!(mode, SslMode::Prefer);
    }

    #[test]
    fn maps_all_supported_values() {
        assert_eq!(parse_ssl_mode("disable").unwrap(), SslMode::Disable);
        assert_eq!(parse_ssl_mode("allow").unwrap(), SslMode::Prefer);
        assert_eq!(parse_ssl_mode("prefer").unwrap(), SslMode::Prefer);
        assert_eq!(parse_ssl_mode("require").unwrap(), SslMode::Require);
        assert_eq!(parse_ssl_mode("verify-ca").unwrap(), SslMode::Require);
        assert_eq!(parse_ssl_mode("VERIFY-FULL").unwrap(), SslMode::Require);
        assert!(parse_ssl_mode("bogus").is_err());
    }

    #[test]
    fn strips_sslmode_and_keeps_other_params() {
        let (url, mode) =
            split_ssl_mode("postgres://u@h:5432/db?sslmode=verify-full&application_name=dbm")
                .unwrap();
        assert_eq!(mode, SslMode::Require);
        assert!(!url.contains("sslmode"));
        assert!(url.contains("application_name=dbm"));
    }

    #[test]
    fn rejects_invalid_url() {
        assert!(split_ssl_mode("not a url").is_err());
    }
}
