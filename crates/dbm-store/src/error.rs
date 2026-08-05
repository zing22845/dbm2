use thiserror::Error;

use dbm_core::ApplicationError;

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("already exists: {0}")]
    AlreadyExists(String),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("registration precheck failed:\n{report}")]
    PrecheckFailed { report: String },

    #[error("{0}")]
    Other(String),
}

impl From<StoreError> for ApplicationError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::NotFound(msg) => ApplicationError::NotFound(msg),
            StoreError::AlreadyExists(msg) => ApplicationError::AlreadyExists(msg),
            StoreError::Sqlite(e) => ApplicationError::Store(e.to_string()),
            StoreError::Io(e) => ApplicationError::Store(e.to_string()),
            StoreError::Json(e) => ApplicationError::Store(e.to_string()),
            StoreError::Crypto(msg) => ApplicationError::Store(msg),
            StoreError::PrecheckFailed { report } => ApplicationError::Store(report),
            StoreError::Other(msg) => ApplicationError::Other(msg),
        }
    }
}
