//! Shared domain types for DBM.

mod connect;
mod connection_pool;
mod driver;
mod engine;
mod error;
mod query;

pub use connect::ConnectOpts;
pub use connection_pool::{ConnectionPoolManager, PoolKey};
pub use driver::{
    ConnectionPool, DatabaseDriver, ForeignKeyInfo, FullDatabaseDriver, SchemaIntrospector,
    TableInfo,
};
pub use engine::Engine;
pub use error::{ApplicationError, DbmError, ErrorSeverity, Result};
pub use query::{ColumnMeta, QueryResult, Row};
