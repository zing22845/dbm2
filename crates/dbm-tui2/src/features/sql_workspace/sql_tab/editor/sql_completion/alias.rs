//! Table-alias suggestion generation.

use std::collections::HashSet;

use super::context::TableRef;
use super::match_score::matches_prefix;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasSuggestion {
    pub label: String,
    pub insert_text: String,
    pub detail: String,
}

const ALIAS_RESERVED: &[&str] = &[
    "all",
    "alter",
    "and",
    "any",
    "as",
    "asc",
    "begin",
    "between",
    "by",
    "case",
    "check",
    "commit",
    "constraint",
    "create",
    "cross",
    "default",
    "delete",
    "desc",
    "distinct",
    "drop",
    "else",
    "end",
    "except",
    "exists",
    "for",
    "foreign",
    "from",
    "full",
    "grant",
    "group",
    "having",
    "in",
    "index",
    "inner",
    "insert",
    "intersect",
    "into",
    "is",
    "join",
    "left",
    "like",
    "limit",
    "natural",
    "not",
    "null",
    "offset",
    "on",
    "or",
    "order",
    "outer",
    "primary",
    "references",
    "right",
    "rollback",
    "select",
    "set",
    "table",
    "then",
    "union",
    "unique",
    "update",
    "using",
    "values",
    "view",
    "when",
    "where",
    "with",
];

pub fn build_alias_items(prefix: &str, referenced: &[TableRef]) -> Vec<AliasSuggestion> {
    let existing: HashSet<String> = referenced
        .iter()
        .filter_map(|t| t.alias.as_ref())
        .map(|a| a.to_ascii_lowercase())
        .collect();
    let mut seen = existing.clone();
    let mut items = Vec::new();

    for table in referenced {
        if table.alias.is_some() {
            continue;
        }
        if !prefix.is_empty() && !matches_prefix(&table.name, prefix) {
            continue;
        }
        let Some(candidate) = generate_alias(&table.name, &seen) else {
            continue;
        };
        seen.insert(candidate.to_ascii_lowercase());
        items.push(AliasSuggestion {
            label: candidate.clone(),
            insert_text: format!("AS {candidate} "),
            detail: format!("alias for {}", table.name),
        });
    }

    items
}

fn generate_alias(table_name: &str, existing: &HashSet<String>) -> Option<String> {
    for candidate in build_alias_candidates(table_name) {
        if !alias_conflicts(&candidate, existing) {
            return Some(candidate);
        }
    }
    for index in 2..100 {
        let candidate = format!("tb{index}");
        if !alias_conflicts(&candidate, existing) {
            return Some(candidate);
        }
    }
    None
}

fn build_alias_candidates(table_name: &str) -> Vec<String> {
    let parts = identifier_words(table_name);
    let mut candidates = Vec::new();

    if parts.len() > 1 {
        let initials: String = parts.iter().filter_map(|p| p.chars().next()).collect();
        if initials.len() >= 2 {
            candidates.push(initials.chars().take(2).collect());
        }
        if initials.len() >= 3 {
            candidates.push(initials.chars().take(3).collect());
        }
        if let Some(first) = parts.first() {
            if first.len() >= 2 {
                candidates.push(first.chars().take(2).collect());
            }
            if first.len() >= 3 {
                candidates.push(first.chars().take(3).collect());
            }
        }
    } else {
        let name: String = if parts.is_empty() {
            table_name
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase()
        } else {
            parts[0].clone()
        };
        let chars: Vec<char> = name.chars().collect();
        if chars.len() <= 3 && !name.is_empty() {
            candidates.push(name.clone());
        }
        if chars.len() >= 2 {
            candidates.push(chars.iter().take(2).collect());
        }
        if chars.len() >= 3 {
            candidates.push(chars.iter().take(3).collect());
        }
        if name.is_empty() {
            candidates.push("tb".to_string());
        }
    }

    candidates
}

fn identifier_words(candidate: &str) -> Vec<String> {
    let mut normalized = String::new();
    let chars: Vec<char> = candidate.chars().collect();
    for (index, ch) in chars.iter().enumerate() {
        if index > 0 && ch.is_ascii_uppercase() && chars[index - 1].is_ascii_lowercase() {
            normalized.push('_');
        }
        normalized.push(ch.to_ascii_lowercase());
    }
    normalized
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn alias_conflicts(candidate: &str, existing: &HashSet<String>) -> bool {
    let lower = candidate.to_ascii_lowercase();
    existing.contains(&lower) || ALIAS_RESERVED.iter().any(|w| *w == lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_alias_for_users() {
        let tables = vec![TableRef {
            name: "users".into(),
            schema: None,
            alias: None,
        }];
        let items = build_alias_items("", &tables);
        assert!(!items.is_empty());
        assert!(items[0].insert_text.starts_with("AS "));
    }

    #[test]
    fn skips_tables_that_already_have_alias() {
        let tables = vec![TableRef {
            name: "users".into(),
            schema: None,
            alias: Some("u".into()),
        }];
        assert!(build_alias_items("", &tables).is_empty());
    }
}
