//! SQL workspace feature state.

use super::sql_tab::state::SqlTabState;

/// State for the SQL workspace. Holds the `sql_tab` child feature state.
#[derive(Debug, Default, Clone)]
pub struct SqlState {
    /// The `sql_tab` parent feature state.
    pub sql_tab: SqlTabState,
}
