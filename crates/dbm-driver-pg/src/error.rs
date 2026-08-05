use std::error::Error as StdError;

use dbm_core::{DbmError, ErrorSeverity};
use tokio_postgres::error::Severity;

/// Formatted Postgres error with severity taken from the server fields.
pub struct FormattedPostgresError {
    pub message: String,
    pub severity: ErrorSeverity,
}

/// Extract a user-facing message + severity from a PostgreSQL driver error.
pub fn format_postgres_error(err: &tokio_postgres::Error) -> FormattedPostgresError {
    if let Some(db_err) = err.as_db_error() {
        let mut parts = vec![db_err.message().trim().to_string()];
        if let Some(detail) = db_err.detail() {
            parts.push(format!("DETAIL: {detail}"));
        }
        if let Some(hint) = db_err.hint() {
            parts.push(format!("HINT: {hint}"));
        }
        return FormattedPostgresError {
            message: parts.join("\n"),
            severity: severity_from_db_error(db_err),
        };
    }

    let msg = err.to_string();
    let message = if msg == "db error" || msg.is_empty() {
        if let Some(source) = err.source() {
            let source_msg = source.to_string();
            if !source_msg.is_empty() && source_msg != "db error" {
                source_msg
            } else {
                msg
            }
        } else {
            msg
        }
    } else {
        msg
    };
    FormattedPostgresError {
        message,
        severity: ErrorSeverity::Error,
    }
}

pub fn dbm_error_from_postgres(err: &tokio_postgres::Error) -> DbmError {
    let formatted = format_postgres_error(err);
    DbmError::database_with_severity(formatted.message, formatted.severity)
}

fn severity_from_db_error(db_err: &tokio_postgres::error::DbError) -> ErrorSeverity {
    if let Some(sev) = db_err.parsed_severity() {
        return severity_from_parsed(sev);
    }
    // Pre-9.6 / unparsed: use the severity field string (may be localized).
    severity_from_label(db_err.severity())
}

fn severity_from_parsed(sev: Severity) -> ErrorSeverity {
    match sev {
        Severity::Panic | Severity::Fatal | Severity::Error => ErrorSeverity::Error,
        Severity::Warning => ErrorSeverity::Warning,
        Severity::Notice | Severity::Debug | Severity::Info | Severity::Log => ErrorSeverity::Info,
    }
}

fn severity_from_label(label: &str) -> ErrorSeverity {
    match label.to_ascii_uppercase().as_str() {
        "WARNING" => ErrorSeverity::Warning,
        "NOTICE" | "INFO" | "LOG" | "DEBUG" => ErrorSeverity::Info,
        _ => ErrorSeverity::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_from_label_maps_pg_fields() {
        assert_eq!(severity_from_label("ERROR"), ErrorSeverity::Error);
        assert_eq!(severity_from_label("FATAL"), ErrorSeverity::Error);
        assert_eq!(severity_from_label("WARNING"), ErrorSeverity::Warning);
        assert_eq!(severity_from_label("NOTICE"), ErrorSeverity::Info);
    }

    #[test]
    fn severity_from_parsed_maps_tokio_severity() {
        assert_eq!(
            severity_from_parsed(Severity::Error),
            ErrorSeverity::Error
        );
        assert_eq!(
            severity_from_parsed(Severity::Warning),
            ErrorSeverity::Warning
        );
        assert_eq!(severity_from_parsed(Severity::Notice), ErrorSeverity::Info);
    }
}
