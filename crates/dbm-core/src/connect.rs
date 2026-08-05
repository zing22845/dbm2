use std::fmt;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::{DbmError, Engine, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectOpts {
    pub url: String,
    pub label: Option<String>,
}

impl ConnectOpts {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            label: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn parse(&self) -> Result<ParsedConnectOpts> {
        ParsedConnectOpts::from_url(&self.url)
    }
}

#[derive(Debug, Clone)]
pub struct ParsedConnectOpts {
    pub engine: Engine,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub database: String,
    pub url: String,
}

impl ParsedConnectOpts {
    pub fn from_url(raw: &str) -> Result<Self> {
        let url = Url::parse(raw).map_err(|e| DbmError::InvalidUrl(e.to_string()))?;
        let scheme = url.scheme();
        let engine = match scheme {
            "postgres" | "postgresql" => Engine::Postgres,
            other => {
                return Err(DbmError::InvalidUrl(format!(
                    "unsupported scheme `{other}` (use postgres:// or postgresql://)"
                )));
            }
        };

        let host = url.host_str().unwrap_or("localhost").to_string();
        let port = url.port().unwrap_or(5432);
        let user = if url.username().is_empty() {
            "postgres".to_string()
        } else {
            url.username().to_string()
        };
        let database = url.path().trim_start_matches('/').to_string();
        let database = if database.is_empty() {
            user.clone()
        } else {
            database.to_string()
        };

        Ok(Self {
            engine,
            host,
            port,
            user,
            database,
            url: raw.to_string(),
        })
    }
}

impl fmt::Display for ParsedConnectOpts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}://{}@{}:{}/{}",
            self.engine, self.user, self.host, self.port, self.database
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_postgres_url() {
        let parsed =
            ParsedConnectOpts::from_url("postgresql://postgres:secret@192.168.64.17:5432/postgres")
                .unwrap();
        assert_eq!(parsed.engine, Engine::Postgres);
        assert_eq!(parsed.host, "192.168.64.17");
        assert_eq!(parsed.port, 5432);
        assert_eq!(parsed.user, "postgres");
        assert_eq!(parsed.database, "postgres");
    }
}
