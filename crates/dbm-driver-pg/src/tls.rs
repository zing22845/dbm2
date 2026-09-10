//! TLS support for PostgreSQL connections.
//!
//! Every pool is built with a rustls connector; the connection's libpq-style
//! `sslmode` decides whether TLS is used and whether the server certificate is
//! verified. The mapping mirrors libpq:
//!
//! | `sslmode`                   | TLS           | certificate      |
//! |-----------------------------|---------------|------------------|
//! | `disable`                   | no            | –                |
//! | `allow` / `prefer`          | if available  | not verified     |
//! | `require`                   | required      | not verified     |
//! | `verify-ca` / `verify-full` | required      | verified         |
//!
//! Verification uses the Mozilla root set (`webpki-roots`); when
//! `DBM_SSLROOTCERT` names a PEM file, its certificates are added as trust
//! anchors too, which is how self-signed or private-CA servers are supported.

use std::path::PathBuf;
use std::sync::Arc;

use dbm_core::{DbmError, Result};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use tokio_postgres::config::SslMode;
use tokio_postgres_rustls::MakeRustlsConnect;

/// Environment variable pointing at a PEM file with extra trust anchors.
pub const ROOT_CERT_ENV: &str = "DBM_SSLROOTCERT";

/// How a PostgreSQL connection should use TLS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgSslMode {
    /// No TLS at all.
    Disable,
    /// Use TLS when the server offers it, fall back to plaintext otherwise.
    Prefer,
    /// Require TLS, but do not verify the server certificate (libpq's
    /// `require`).
    Require,
    /// Require TLS and verify the server certificate (chain + hostname).
    Verify,
}

impl PgSslMode {
    /// The mode handed to `tokio_postgres::Config`.
    pub fn tokio_ssl_mode(self) -> SslMode {
        match self {
            Self::Disable => SslMode::Disable,
            Self::Prefer => SslMode::Prefer,
            Self::Require | Self::Verify => SslMode::Require,
        }
    }
}

/// Extract `sslmode` from a connection URL and map it to a [`PgSslMode`].
///
/// The parameter is stripped from the returned URL because tokio-postgres
/// rejects values outside libpq's `disable`/`prefer`/`require` set (notably
/// `verify-full`), which we translate ourselves.
pub fn split_ssl_mode(raw_url: &str) -> Result<(String, PgSslMode)> {
    let mut url = url::Url::parse(raw_url).map_err(|e| DbmError::InvalidUrl(e.to_string()))?;

    let mut mode = PgSslMode::Prefer;
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

/// Map a libpq `sslmode` value onto a [`PgSslMode`].
pub fn parse_ssl_mode(raw: &str) -> Result<PgSslMode> {
    let normalized = raw.trim().to_ascii_lowercase();
    Ok(match normalized.as_str() {
        "disable" => PgSslMode::Disable,
        // `allow` has no tokio-postgres equivalent; treat it as `prefer`.
        "allow" | "prefer" | "" => PgSslMode::Prefer,
        "require" => PgSslMode::Require,
        // `verify-ca` would skip the hostname check; rustls always checks it,
        // so both verification levels behave like `verify-full`.
        "verify-ca" | "verify-full" => PgSslMode::Verify,
        other => {
            return Err(DbmError::InvalidUrl(format!(
                "unsupported sslmode `{other}` (expected disable, allow, prefer, require, verify-ca or verify-full)"
            )));
        }
    })
}

/// Build the rustls connector for the given mode.
///
/// `Verify` trusts the bundled Mozilla roots plus `DBM_SSLROOTCERT` (when set);
/// every other mode accepts any certificate, because with those modes the
/// connection is only encrypted, not authenticated.
pub fn connector(mode: PgSslMode) -> Result<MakeRustlsConnect> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .expect("ring always supports the default protocol versions");

    let config = match mode {
        PgSslMode::Verify => {
            let mut roots = RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            if let Some(path) = root_cert_path() {
                let certs = load_root_certs(&path)?;
                let (added, ignored) = roots.add_parsable_certificates(certs);
                if added == 0 {
                    return Err(DbmError::database(format!(
                        "no usable certificates found in {} ({} ignored)",
                        path.display(),
                        ignored
                    )));
                }
            }
            builder.with_root_certificates(roots).with_no_client_auth()
        }
        PgSslMode::Disable | PgSslMode::Prefer | PgSslMode::Require => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoCertificateVerification(provider)))
            .with_no_client_auth(),
    };

    Ok(MakeRustlsConnect::new(config))
}

/// Path from [`ROOT_CERT_ENV`], when set to a non-empty value.
fn root_cert_path() -> Option<PathBuf> {
    std::env::var(ROOT_CERT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn load_root_certs(path: &std::path::Path) -> Result<Vec<CertificateDer<'static>>> {
    let file = std::fs::File::open(path)
        .map_err(|e| DbmError::database(format!("failed to open {}: {e}", path.display())))?;
    let mut reader = std::io::BufReader::new(file);
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| DbmError::database(format!("failed to parse {}: {e}", path.display())))?;
    if certs.is_empty() {
        return Err(DbmError::database(format!(
            "{} contains no PEM certificates",
            path.display()
        )));
    }
    Ok(certs)
}

/// Certificate verifier that accepts everything — used by the modes that
/// encrypt without authenticating the server.
#[derive(Debug)]
struct NoCertificateVerification(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_sslmode_defaults_to_prefer() {
        assert_eq!(parse_ssl_mode("").unwrap(), PgSslMode::Prefer);
        let (url, mode) = split_ssl_mode("postgres://u@h:5432/db").unwrap();
        assert_eq!(url, "postgres://u@h:5432/db");
        assert_eq!(mode, PgSslMode::Prefer);
    }

    #[test]
    fn maps_all_supported_values() {
        assert_eq!(parse_ssl_mode("disable").unwrap(), PgSslMode::Disable);
        assert_eq!(parse_ssl_mode("allow").unwrap(), PgSslMode::Prefer);
        assert_eq!(parse_ssl_mode("prefer").unwrap(), PgSslMode::Prefer);
        assert_eq!(parse_ssl_mode("require").unwrap(), PgSslMode::Require);
        assert_eq!(parse_ssl_mode("verify-ca").unwrap(), PgSslMode::Verify);
        assert_eq!(parse_ssl_mode("VERIFY-FULL").unwrap(), PgSslMode::Verify);
        assert!(parse_ssl_mode("bogus").is_err());
    }

    #[test]
    fn tokio_ssl_mode_mapping() {
        assert_eq!(PgSslMode::Disable.tokio_ssl_mode(), SslMode::Disable);
        assert_eq!(PgSslMode::Prefer.tokio_ssl_mode(), SslMode::Prefer);
        assert_eq!(PgSslMode::Require.tokio_ssl_mode(), SslMode::Require);
        // Verification only changes the certificate policy, not TLS usage.
        assert_eq!(PgSslMode::Verify.tokio_ssl_mode(), SslMode::Require);
    }

    #[test]
    fn strips_sslmode_and_keeps_other_params() {
        let (url, mode) =
            split_ssl_mode("postgres://u@h:5432/db?sslmode=verify-full&application_name=dbm")
                .unwrap();
        assert_eq!(mode, PgSslMode::Verify);
        assert!(!url.contains("sslmode"));
        assert!(url.contains("application_name=dbm"));
    }

    #[test]
    fn rejects_invalid_url() {
        assert!(split_ssl_mode("not a url").is_err());
    }

    #[test]
    fn connector_builds_for_every_mode() {
        for mode in [
            PgSslMode::Disable,
            PgSslMode::Prefer,
            PgSslMode::Require,
            PgSslMode::Verify,
        ] {
            assert!(connector(mode).is_ok(), "mode {mode:?} should build");
        }
    }
}
