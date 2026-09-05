//! PostgreSQL driver implementation.

mod client;
mod convert;
mod driver;
mod error;
mod ident;
mod pagination;
mod pool;

pub use driver::PostgresDriver;
pub use error::{FormattedPostgresError, dbm_error_from_postgres, format_postgres_error};

pub use client::PostgresClient;
pub use pagination::{count_select_sql, is_read_only, paginated_select_sql, user_result_row_cap};
pub use pool::{PRIMARY_KEYS_SQL, PostgresPool};
