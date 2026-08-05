use super::engine::SqlEngine;

const COMMON_KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "JOIN",
    "LEFT",
    "RIGHT",
    "INNER",
    "OUTER",
    "ON",
    "GROUP BY",
    "ORDER BY",
    "ASC",
    "DESC",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "TABLE",
    "VIEW",
    "AS",
    "AND",
    "OR",
    "NOT",
    "IN",
    "IS",
    "NULL",
    "LIKE",
    "DISTINCT",
    "UNION",
    "ALL",
    "EXISTS",
    "BETWEEN",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "COALESCE",
    "CAST",
    "ALTER",
    "DROP",
    "ADD",
    "COLUMN",
    "INDEX",
    "PRIMARY",
    "KEY",
    "FOREIGN",
    "REFERENCES",
    "CONSTRAINT",
    "DEFAULT",
    "CHECK",
    "UNIQUE",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "TRUNCATE",
    "EXPLAIN",
    "ANALYZE",
    "WITH",
    "RECURSIVE",
    "CROSS",
    "FULL",
    "NATURAL",
    "USING",
    "RETURNING",
];

const POSTGRES_KEYWORDS: &[&str] = &[
    "ILIKE",
    "SERIAL",
    "BIGSERIAL",
    "SMALLSERIAL",
    "JSON",
    "JSONB",
    "UUID",
    "BYTEA",
    "BOOLEAN",
    "MATERIALIZED",
    "ON CONFLICT",
    "DO NOTHING",
    "DO UPDATE",
    "ARRAY_AGG",
    "CURRENT_TIMESTAMP",
];

const HIGH_FREQUENCY: &[&str] = &[
    "SELECT", "FROM", "WHERE", "AND", "OR", "JOIN", "ON", "IN", "AS", "GROUP BY", "ORDER BY",
    "LEFT", "RIGHT", "INNER", "OUTER", "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE",
    "NOT", "NULL", "LIMIT", "DISTINCT",
];

/// Clause introducers after a resolved table / row source (DBX-style), not the full keyword list.
pub const CLAUSE_KEYWORDS: &[&str] = &[
    "WHERE",
    "JOIN",
    "LEFT",
    "RIGHT",
    "INNER",
    "OUTER",
    "CROSS",
    "FULL",
    "NATURAL",
    "ON",
    "USING",
    "GROUP BY",
    "ORDER BY",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "UNION",
    "ALL",
    "EXCEPT",
    "INTERSECT",
    "FOR",
    "RETURNING",
];

pub fn keywords_for_engine(engine: SqlEngine) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = COMMON_KEYWORDS.to_vec();
    match engine {
        SqlEngine::Postgres => out.extend(POSTGRES_KEYWORDS),
    }
    out
}

pub fn keyword_rank(keyword: &str) -> u8 {
    let upper = keyword.to_ascii_uppercase();
    if HIGH_FREQUENCY
        .iter()
        .any(|k| k.eq_ignore_ascii_case(&upper))
    {
        0
    } else {
        1
    }
}
