use thiserror::Error;

pub type Result<T> = std::result::Result<T, DbmError>;

/// User-facing severity for status / chrome coloring.
///
/// For PostgreSQL errors this mirrors `DbError` severity (ERROR/FATAL/PANIC →
/// [`Error`], WARNING → [`Warning`], NOTICE/INFO/… → [`Info`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorSeverity {
    Info,
    Warning,
    #[default]
    Error,
}

#[derive(Debug, Error)]
pub enum DbmError {
    #[error("invalid connection URL: {0}")]
    InvalidUrl(String),

    #[error("database error: {message}")]
    Database {
        message: String,
        severity: ErrorSeverity,
    },

    #[error("{0}")]
    Other(String),
}

impl DbmError {
    pub fn database(message: impl Into<String>) -> Self {
        Self::database_with_severity(message, ErrorSeverity::Error)
    }

    pub fn database_with_severity(message: impl Into<String>, severity: ErrorSeverity) -> Self {
        Self::Database {
            message: message.into(),
            severity,
        }
    }

    /// User-facing message without error-kind prefix.
    pub fn user_message(&self) -> &str {
        match self {
            Self::InvalidUrl(message) | Self::Other(message) => message,
            Self::Database { message, .. } => message,
        }
    }

    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::Database { severity, .. } => *severity,
            Self::InvalidUrl(_) | Self::Other(_) => ErrorSeverity::Error,
        }
    }
}

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("invalid connection URL: {0}")]
    InvalidUrl(String),

    #[error("database error: {message}")]
    Database {
        message: String,
        severity: ErrorSeverity,
    },

    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    #[error("query execution failed: {0}")]
    QueryFailed(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("timeout after {duration_ms}ms: {operation}")]
    Timeout { operation: String, duration_ms: u64 },

    #[error("store error: {0}")]
    Store(String),

    #[error("driver error [{engine}]: {message}")]
    Driver { engine: String, message: String },

    #[error("not found: {0}")]
    NotFound(String),

    #[error("already exists: {0}")]
    AlreadyExists(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("{0}")]
    Other(String),
}

impl ApplicationError {
    pub fn user_message(&self) -> &str {
        match self {
            Self::InvalidUrl(msg) => msg,
            Self::Database { message, .. } => message,
            Self::ConnectionFailed(msg) => msg,
            Self::QueryFailed(msg) => msg,
            Self::PermissionDenied(msg) => msg,
            Self::Timeout { operation, .. } => operation,
            Self::Store(msg) => msg,
            Self::Driver { message, .. } => message,
            Self::NotFound(msg) => msg,
            Self::AlreadyExists(msg) => msg,
            Self::Internal(msg) => msg,
            Self::Other(msg) => msg,
        }
    }

    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::Database { severity, .. } => *severity,
            Self::InvalidUrl(_)
            | Self::ConnectionFailed(_)
            | Self::QueryFailed(_)
            | Self::PermissionDenied(_)
            | Self::Timeout { .. }
            | Self::Internal(_)
            | Self::Driver { .. } => ErrorSeverity::Error,
            Self::Store(_) | Self::NotFound(_) | Self::AlreadyExists(_) | Self::Other(_) => {
                ErrorSeverity::Error
            }
        }
    }
}

impl From<DbmError> for ApplicationError {
    fn from(err: DbmError) -> Self {
        match err {
            DbmError::InvalidUrl(msg) => Self::InvalidUrl(msg),
            DbmError::Database { message, severity } => Self::Database { message, severity },
            DbmError::Other(msg) => Self::Other(msg),
        }
    }
}

impl From<ApplicationError> for DbmError {
    fn from(err: ApplicationError) -> Self {
        match err {
            ApplicationError::InvalidUrl(msg) => DbmError::InvalidUrl(msg),
            ApplicationError::Database { message, severity } => {
                DbmError::Database { message, severity }
            }
            ApplicationError::ConnectionFailed(msg) => DbmError::Other(msg),
            ApplicationError::QueryFailed(msg) => DbmError::Other(msg),
            ApplicationError::PermissionDenied(msg) => DbmError::Other(msg),
            ApplicationError::Timeout {
                operation,
                duration_ms,
            } => DbmError::Other(format!("timeout after {duration_ms}ms: {operation}")),
            ApplicationError::Store(msg) => DbmError::Other(msg),
            ApplicationError::Driver { engine, message } => {
                DbmError::Other(format!("[{engine}] {message}"))
            }
            ApplicationError::NotFound(msg) => DbmError::Other(msg),
            ApplicationError::AlreadyExists(msg) => DbmError::Other(msg),
            ApplicationError::Internal(msg) => DbmError::Other(msg),
            ApplicationError::Other(msg) => DbmError::Other(msg),
        }
    }
}
