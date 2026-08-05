const RESERVED_ALIASES: &[&str] = &[
    "where",
    "group",
    "order",
    "having",
    "limit",
    "offset",
    "union",
    "intersect",
    "except",
    "and",
    "or",
    "not",
    "is",
    "like",
    "in",
    "between",
    "exists",
    "select",
    "on",
    "set",
    "left",
    "right",
    "inner",
    "outer",
    "cross",
    "full",
    "natural",
    "join",
    "from",
    "as",
    "values",
    "returning",
    "insert",
    "update",
    "delete",
    "into",
    "using",
    "by",
    "distinct",
    "case",
    "when",
    "then",
    "else",
    "end",
    "with",
    "apply",
];

pub fn is_alias_blacklisted(alias: &str) -> bool {
    let lower = alias.to_ascii_lowercase();
    RESERVED_ALIASES.contains(&lower.as_str())
}

/// Clause keywords users type letter-by-letter; excludes `USING` etc. that collide with real aliases.
const CLAUSE_TYPING_TARGETS: &[&str] = &[
    "WHERE",
    "JOIN",
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

pub fn is_clause_typing_prefix(word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let lower = word.to_ascii_lowercase();
    if is_alias_blacklisted(&lower) {
        return true;
    }
    if lower.len() == 1 {
        return matches!(lower.as_str(), "w" | "j" | "g" | "o" | "h" | "l");
    }
    CLAUSE_TYPING_TARGETS.iter().any(|kw| {
        let kl = kw.to_ascii_lowercase();
        kl.starts_with(&lower) && lower.len() < kl.len()
    })
}

pub fn sanitize_table_alias(alias: &str) -> Option<String> {
    if alias.is_empty() || is_alias_blacklisted(alias) || is_clause_typing_prefix(alias) {
        None
    } else {
        Some(alias.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_where_and_join_as_alias() {
        assert!(is_alias_blacklisted("where"));
        assert!(is_alias_blacklisted("JOIN"));
    }

    #[test]
    fn blocks_clause_typing_prefixes() {
        assert!(is_clause_typing_prefix("w"));
        assert!(is_clause_typing_prefix("wh"));
        assert!(!is_clause_typing_prefix("usr"));
        assert!(!is_clause_typing_prefix("u"));
    }

    #[test]
    fn allows_real_alias_us() {
        assert!(!is_clause_typing_prefix("us"));
        assert_eq!(sanitize_table_alias("us"), Some("us".into()));
    }
}
