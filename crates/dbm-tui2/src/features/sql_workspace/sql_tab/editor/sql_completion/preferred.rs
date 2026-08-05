//! Preferred-keyword selection: which keywords rank at the top for a given
//! SQL position (e.g. `VALUES` after an INSERT column list, `AND`/`OR` in a
//! WHERE condition, `SET`/`WHERE` for UPDATE/DELETE).

use crate::common::utils::cursor::Cursor;

use super::context::is_select_list_column_context;
use super::ident;

const SELECT_BODY_INCOMPLETE_TAIL: &[&str] = &[
    "where", "and", "or", "not", "having", "group", "order", "by", "on", "is", "in", "like",
    "between",
];

const CONDITION_INCOMPLETE_TAIL: &[&str] = &[
    "where", "and", "or", "not", "having", "on", "is", "in", "like", "between", "exists",
];

pub struct UpdateContext {
    pub after_target: bool,
    pub after_set_assignments: bool,
}

pub struct DeleteContext {
    pub after_target: bool,
}

pub fn preferred_keywords_for_completion(
    sql: &str,
    cursor: Cursor,
    exclusive_table_suggestions: bool,
) -> Vec<&'static str> {
    let offset = super::context::cursor_offset(sql, cursor);
    let stmt_start = ident::statement_start_before(sql, offset);
    let before_cursor = &sql[stmt_start..offset];
    let before_token = before_token(before_cursor);
    let select_list = is_select_list_column_context(sql, cursor);
    let update = detect_update_context(before_cursor);
    let delete = detect_delete_context(before_cursor);

    let mut keywords = Vec::new();
    if select_list && has_select_list_expression(before_cursor) {
        keywords.push("FROM");
    }
    if !exclusive_table_suggestions && is_after_select_body_expression(&before_token) {
        keywords.push("LIMIT");
    }
    if is_after_condition_expression(&before_token) {
        keywords.extend(["AND", "OR"]);
    }
    if let Some(info) = &update {
        if info.after_target {
            keywords.push("SET");
        }
        if info.after_set_assignments {
            keywords.push("WHERE");
        }
    }
    if let Some(info) = &delete
        && info.after_target
    {
        keywords.push("WHERE");
    }
    if super::context::is_after_insert_column_list(before_cursor) {
        keywords.push("VALUES");
    }
    keywords
}

fn before_token(before_cursor: &str) -> String {
    super::context::stmt_before_trailing_ident(
        before_cursor,
        Cursor::new(0, before_cursor.chars().count()),
    )
}

fn strip_sql_literals(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\'' {
            out.push('\'');
            out.push('\'');
            while let Some(next) = chars.next() {
                if next == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                        continue;
                    }
                    break;
                }
            }
            continue;
        }
        if ch == '"' {
            out.push('"');
            out.push('"');
            while let Some(next) = chars.next() {
                if next == '"' {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        continue;
                    }
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn has_select_list_expression(before_cursor: &str) -> bool {
    let stripped = strip_sql_literals(before_cursor);
    let cleaned = stripped.trim_end();
    let lower = cleaned.to_ascii_lowercase();
    let select_index = last_top_level_keyword_index(&lower, "select");
    if select_index < 0 {
        return false;
    }
    let after_select = cleaned[select_index as usize + "select".len()..].trim();
    !after_select.is_empty() && !after_select.eq_ignore_ascii_case("distinct")
}

fn is_after_select_body_expression(before_token: &str) -> bool {
    let stripped = strip_sql_literals(before_token);
    let cleaned = stripped.trim_end();
    let lower = cleaned.to_ascii_lowercase();
    if !lower.starts_with("select") && !lower.contains(" select ") {
        return false;
    }
    if !lower.contains(" from ") && !lower.ends_with(" from") {
        return false;
    }
    if lower.contains(" limit ")
        || lower.contains(" union ")
        || lower.contains(" intersect ")
        || lower.contains(" except ")
        || lower.contains(" offset ")
    {
        return false;
    }
    if let Some(last) = last_ascii_word(cleaned)
        && SELECT_BODY_INCOMPLETE_TAIL
            .iter()
            .any(|kw| last.eq_ignore_ascii_case(kw))
    {
        return false;
    }
    true
}

fn is_after_condition_expression(before_token: &str) -> bool {
    let stripped = strip_sql_literals(before_token);
    let cleaned = stripped.trim_end();
    if !has_active_condition_clause(cleaned) {
        return false;
    }
    if let Some(last) = last_ascii_word(cleaned)
        && CONDITION_INCOMPLETE_TAIL
            .iter()
            .any(|kw| last.eq_ignore_ascii_case(kw))
    {
        return false;
    }
    is_expression_tail_complete(cleaned)
}

fn has_active_condition_clause(sql: &str) -> bool {
    let lower = sql.to_ascii_lowercase();
    let where_index = last_top_level_keyword_index(&lower, "where");
    let having_index = last_top_level_keyword_index(&lower, "having");
    let on_index = last_top_level_keyword_index(&lower, "on");
    let condition_index = where_index.max(having_index).max(on_index);
    if condition_index < 0 {
        return false;
    }
    let after = &lower[condition_index as usize..];
    !after.contains("group by")
        && !after.contains("order by")
        && !after.contains(" limit ")
        && !after.contains(" union ")
        && !after.contains(" intersect ")
        && !after.contains(" except ")
        && !after.contains(" offset ")
}

fn is_expression_tail_complete(sql: &str) -> bool {
    let trimmed = sql.trim_end();
    if trimmed.is_empty() {
        return false;
    }
    let last_char = trimmed.chars().last().unwrap_or(' ');
    if matches!(
        last_char,
        ',' | '.' | '(' | '+' | '-' | '*' | '/' | '%' | '<' | '>' | '=' | '!' | '&' | '|'
    ) {
        return false;
    }
    if trimmed.ends_with(')') || trimmed.ends_with(']') {
        return true;
    }
    if trimmed.ends_with("true")
        || trimmed.ends_with("false")
        || trimmed.ends_with("null")
        || trimmed.ends_with("''")
    {
        return true;
    }
    last_ascii_word(trimmed).is_some()
}

fn last_top_level_keyword_index(lower: &str, keyword: &str) -> i32 {
    let mut depth = 0i32;
    let mut last_index = -1i32;
    let mut byte_index = 0usize;
    while byte_index < lower.len() {
        let rest = &lower[byte_index..];
        let Some(ch) = rest.chars().next() else {
            break;
        };
        let ch_len = ch.len_utf8();
        if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth = (depth - 1).max(0);
        } else if depth == 0 && rest.starts_with(keyword) {
            let before = lower[..byte_index].chars().last();
            let after_pos = byte_index + keyword.len();
            let after = lower[after_pos..].chars().next();
            let before_ok = before.is_none_or(|c| !ident::is_ident_part(c));
            let after_ok = after.is_none_or(|c| !ident::is_ident_part(c));
            if before_ok && after_ok {
                last_index = byte_index as i32;
            }
            byte_index += keyword.len();
            continue;
        }
        byte_index += ch_len;
    }
    last_index
}

fn last_ascii_word(text: &str) -> Option<String> {
    let trimmed = text.trim_end();
    let mut word = String::new();
    for ch in trimmed.chars().rev() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            word.push(ch);
        } else if !word.is_empty() {
            break;
        }
    }
    if word.is_empty() {
        None
    } else {
        Some(word.chars().rev().collect())
    }
}

fn detect_update_context(before_cursor: &str) -> Option<UpdateContext> {
    let stripped = strip_sql_literals(before_cursor);
    let cleaned = stripped.trim_start();
    let lower = cleaned.to_ascii_lowercase();
    if !lower.starts_with("update") {
        return None;
    }
    let rest = cleaned.get(6..)?.trim_start();
    if rest.is_empty() {
        return Some(UpdateContext {
            after_target: false,
            after_set_assignments: false,
        });
    }
    let table_len = ident::scan_ident_end(rest);
    if table_len == 0 {
        return None;
    }
    let after_table = rest.get(table_len..)?.trim_start();
    if after_table.is_empty() {
        return Some(UpdateContext {
            after_target: true,
            after_set_assignments: false,
        });
    }
    if !after_table.contains(' ')
        && after_table
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Some(UpdateContext {
            after_target: true,
            after_set_assignments: false,
        });
    }
    let lower_after = after_table.to_ascii_lowercase();
    if !lower_after.contains("set") {
        return Some(UpdateContext {
            after_target: true,
            after_set_assignments: false,
        });
    }
    let set_idx = lower_after.find("set")?;
    let set_segment = after_table.get(set_idx + 3..)?.trim_start();
    if set_segment.to_ascii_lowercase().contains("where") {
        return Some(UpdateContext {
            after_target: false,
            after_set_assignments: false,
        });
    }
    Some(UpdateContext {
        after_target: false,
        after_set_assignments: set_segment.contains('='),
    })
}

fn detect_delete_context(before_cursor: &str) -> Option<DeleteContext> {
    let stripped = strip_sql_literals(before_cursor);
    let cleaned = stripped.trim_start();
    let lower = cleaned.to_ascii_lowercase();
    if !lower.starts_with("delete") {
        return None;
    }
    let from_idx = lower.find(" from ")?;
    let rest = cleaned[from_idx + " from ".len()..].trim_start();
    let table_end = scan_table_ref_end(rest);
    if table_end == 0 {
        return Some(DeleteContext { after_target: false });
    }
    let after_target = rest[table_end..].trim_start();
    let after_target_only = after_target.is_empty()
        || after_target
            .split_whitespace()
            .next()
            .is_some_and(|w| w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    Some(DeleteContext {
        after_target: after_target_only,
    })
}

fn scan_table_ref_end(input: &str) -> usize {
    let trimmed = input.trim_start();
    let skip = input.len() - trimmed.len();
    skip + ident::scan_ident_end(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor_at(sql: &str) -> Cursor {
        Cursor::new(0, sql.chars().count())
    }

    #[test]
    fn prefers_values_after_insert_column_list() {
        let sql = "INSERT INTO users (id, name) ";
        let preferred = preferred_keywords_for_completion(sql, cursor_at(sql), false);
        assert!(preferred.contains(&"VALUES"));
    }

    #[test]
    fn delete_from_users_wh_prefers_where() {
        let sql = "delete from users wh";
        let preferred = preferred_keywords_for_completion(sql, cursor_at(sql), false);
        assert!(preferred.iter().any(|k| k.eq_ignore_ascii_case("WHERE")));
    }

    #[test]
    fn where_condition_prefers_and_or() {
        let sql = "select * from users where id > 0 ";
        let preferred = preferred_keywords_for_completion(sql, cursor_at(sql), false);
        assert!(preferred.contains(&"AND"));
        assert!(preferred.contains(&"OR"));
    }

    #[test]
    fn select_body_prefers_limit() {
        let sql = "select * from users ";
        let preferred = preferred_keywords_for_completion(sql, cursor_at(sql), false);
        assert!(preferred.contains(&"LIMIT"));
    }
}
