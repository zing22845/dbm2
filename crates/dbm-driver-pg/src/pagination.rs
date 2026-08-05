//! Server-side LIMIT/OFFSET pagination for read-only queries (PostgreSQL).
//!
//! Wraps the user's SELECT in LIMIT/OFFSET for instance-maintenance browsing;
//! see DBX `query_result_sql` for the multi-dialect reference implementation.

/// True when the statement head looks like a read-only query.
pub fn is_read_only(sql: &str) -> bool {
    let head = sql
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        head.as_str(),
        "select" | "with" | "show" | "explain" | "table" | "values"
    )
}

/// Build paginated SQL, or `None` when the input is not a single paginable statement.
///
/// Always wraps the user query so UI page size applies even when the user wrote `LIMIT n`.
pub fn paginated_select_sql(sql: &str, limit: u64, offset: u64) -> Option<String> {
    let statement = single_select_statement(sql)?;
    Some(format!(
        "SELECT * FROM ({statement}) AS dbm_page LIMIT {limit} OFFSET {offset}"
    ))
}

/// Upper bound on rows the user query can return (top-level `LIMIT n`), if present.
pub fn user_result_row_cap(sql: &str) -> Option<u64> {
    let statement = single_select_statement(sql)?;
    let tokens = top_level_tokens(&statement);
    for (i, token) in tokens.iter().enumerate() {
        if token == "LIMIT" {
            let next = tokens.get(i + 1)?;
            return next.parse::<u64>().ok();
        }
    }
    None
}

/// Build `COUNT(*)` over the user query for total page count.
pub fn count_select_sql(sql: &str) -> Option<String> {
    let statement = single_select_statement(sql)?;
    Some(format!(
        "SELECT COUNT(*) AS dbm_total_rows FROM ({statement}) AS dbm_count"
    ))
}

fn single_select_statement(sql: &str) -> Option<String> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    if trimmed.is_empty() || trimmed.contains(';') {
        return None;
    }
    let head = trimmed.split_whitespace().next()?.to_ascii_lowercase();
    if !matches!(head.as_str(), "select" | "with" | "table" | "values") {
        return None;
    }
    Some(trimmed.to_string())
}

fn top_level_tokens(sql: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut depth = 0usize;

    while i < sql.len() {
        let ch = sql[i..].chars().next().unwrap_or('\0');

        if ch == '-' && sql[i..].starts_with("--") {
            i += 2;
            while i < sql.len() && sql[i..].chars().next().unwrap_or('\n') != '\n' {
                i += 1;
            }
            continue;
        }

        if ch == '/' && sql[i..].starts_with("/*") {
            i += 2;
            while i < sql.len() {
                let current = sql[i..].chars().next().unwrap_or('\0');
                let following = sql.get(i + current.len_utf8()..).and_then(|s| s.chars().next());
                i += current.len_utf8();
                if current == '*' && following == Some('/') {
                    i += 1;
                    break;
                }
            }
            continue;
        }

        if matches!(ch, '\'' | '"') {
            i = skip_quoted(sql, i, ch);
            continue;
        }

        if ch == '(' {
            depth += 1;
            i += 1;
            continue;
        }
        if ch == ')' {
            depth = depth.saturating_sub(1);
            i += 1;
            continue;
        }

        if depth == 0 && ch.is_ascii_digit() {
            let start = i;
            i += ch.len_utf8();
            while i < sql.len() {
                let next = sql[i..].chars().next().unwrap_or('\0');
                if next.is_ascii_digit() {
                    i += next.len_utf8();
                } else {
                    break;
                }
            }
            tokens.push(sql[start..i].to_string());
            continue;
        }

        if depth == 0 && (ch.is_ascii_alphabetic() || ch == '_') {
            let start = i;
            i += ch.len_utf8();
            while i < sql.len() {
                let next = sql[i..].chars().next().unwrap_or('\0');
                if next.is_ascii_alphanumeric() || next == '_' {
                    i += next.len_utf8();
                } else {
                    break;
                }
            }
            tokens.push(sql[start..i].to_ascii_uppercase());
            continue;
        }

        i += ch.len_utf8();
    }

    tokens
}

fn skip_quoted(sql: &str, pos: usize, quote: char) -> usize {
    let mut i = pos + quote.len_utf8();
    while i < sql.len() {
        let ch = sql[i..].chars().next().unwrap_or('\0');
        let next = sql.get(i + ch.len_utf8()..).and_then(|s| s.chars().next());
        if ch == quote {
            if next == Some(quote) {
                i += ch.len_utf8() + quote.len_utf8();
                continue;
            }
            return i + ch.len_utf8();
        }
        i += ch.len_utf8();
    }
    sql.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_limit_offset() {
        assert_eq!(
            paginated_select_sql("SELECT id FROM users", 100, 200).unwrap(),
            "SELECT * FROM (SELECT id FROM users) AS dbm_page LIMIT 100 OFFSET 200"
        );
    }

    #[test]
    fn wraps_user_limit_on_first_page() {
        assert_eq!(
            paginated_select_sql("SELECT id FROM users LIMIT 20", 100, 0).unwrap(),
            "SELECT * FROM (SELECT id FROM users LIMIT 20) AS dbm_page LIMIT 100 OFFSET 0"
        );
    }

    #[test]
    fn wraps_user_limit_for_later_pages() {
        assert_eq!(
            paginated_select_sql("SELECT id FROM users LIMIT 20", 5, 10).unwrap(),
            "SELECT * FROM (SELECT id FROM users LIMIT 20) AS dbm_page LIMIT 5 OFFSET 10"
        );
    }

    #[test]
    fn user_result_row_cap_reads_top_level_limit() {
        assert_eq!(
            user_result_row_cap("SELECT * FROM users LIMIT 300"),
            Some(300)
        );
        assert_eq!(user_result_row_cap("SELECT * FROM users"), None);
    }

    #[test]
    fn count_wraps_subquery() {
        assert_eq!(
            count_select_sql("SELECT id FROM users WHERE active").unwrap(),
            "SELECT COUNT(*) AS dbm_total_rows FROM (SELECT id FROM users WHERE active) AS dbm_count"
        );
    }

    #[test]
    fn rejects_multi_statement() {
        assert!(paginated_select_sql("SELECT 1; SELECT 2", 10, 0).is_none());
    }

    #[test]
    fn cte_select_paginates() {
        assert_eq!(
            paginated_select_sql(
                "WITH picked AS (SELECT id FROM users LIMIT 10) SELECT * FROM picked",
                100,
                0
            )
            .unwrap(),
            "SELECT * FROM (WITH picked AS (SELECT id FROM users LIMIT 10) SELECT * FROM picked) AS dbm_page LIMIT 100 OFFSET 0"
        );
    }
}
