/// SQL engine kind for completion metadata and keyword lists.
///
/// New engines (MySQL, SQLite) plug in here without changing the TUI flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlEngine {
    Postgres,
}
