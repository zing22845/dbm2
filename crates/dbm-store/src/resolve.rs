use crate::{Store, StoreError, StoreResult};

#[derive(Debug, Clone)]
pub struct SqlSession {
    pub instance_name: String,
    pub connection_name: String,
    pub display_target: String,
    pub url: String,
}

#[derive(Debug, Default, Clone)]
pub struct SessionResolveOptions {
    pub url: Option<String>,
    pub instance: Option<String>,
    pub connection: Option<String>,
}

/// Resolve a SQL session URL.
///
/// 1. Explicit `--url`
/// 2. `--instance` + `--connection` (both required)
/// 3. `DBM_DATABASE_URL`
/// 4. First managed instance with a connection
pub fn resolve_sql_session(opts: SessionResolveOptions) -> StoreResult<SqlSession> {
    if let Some(url) = opts.url {
        return Ok(SqlSession {
            instance_name: "url".into(),
            connection_name: "url".into(),
            display_target: "custom url".into(),
            url,
        });
    }

    if let Some(instance_name) = opts.instance {
        let store = Store::open_default()?;
        let connection_name = opts.connection.ok_or_else(|| {
            StoreError::Other(format!(
                "instance `{instance_name}` requires --connection (no default connection)"
            ))
        })?;
        let url = store.instance_connection_url(&instance_name, &connection_name)?;
        let instance = store.get_managed_instance_by_name(&instance_name)?;
        let conn = store.get_instance_connection(&instance_name, &connection_name)?;
        return Ok(SqlSession {
            instance_name,
            connection_name,
            display_target: conn.display_target(&instance),
            url,
        });
    }

    if let Ok(url) = std::env::var("DBM_DATABASE_URL") {
        return Ok(SqlSession {
            instance_name: "env".into(),
            connection_name: "env".into(),
            display_target: "DBM_DATABASE_URL".into(),
            url,
        });
    }

    if let Ok(store) = Store::open_default()
        && let Some((instance, conn)) = store.find_first_session()?
    {
        let url = store.instance_connection_url(&instance.name, &conn.name)?;
        return Ok(SqlSession {
            instance_name: instance.name.clone(),
            connection_name: conn.name.clone(),
            display_target: conn.display_target(&instance),
            url,
        });
    }

    Err(crate::StoreError::Other(
        "no SQL session configured: register an instance and add a connection, or pass --url"
            .into(),
    ))
}

pub fn resolve_database_url(opts: SessionResolveOptions) -> StoreResult<String> {
    Ok(resolve_sql_session(opts)?.url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_url_wins() {
        let session = resolve_sql_session(SessionResolveOptions {
            url: Some("postgresql://u@h/db".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(session.url, "postgresql://u@h/db");
    }
}
