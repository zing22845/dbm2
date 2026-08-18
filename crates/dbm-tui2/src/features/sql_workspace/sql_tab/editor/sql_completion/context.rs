//! Completion context analysis (editor-agnostic).
//!
//! Turns `(sql, cursor)` into a [`CompletionContext`] describing what the user
//! is completing (table / column / keyword / suppressed). Pure and cursor-based
//! via the [`Cursor`] abstraction; the semantic model adds DBX-aligned
//! refinement on top of a heuristic pass.

use crate::common::utils::cursor::Cursor;

use super::alias_blacklist::sanitize_table_alias;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    pub name: String,
    pub schema: Option<String>,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionIntent {
    Suppressed,
    Keyword,
    Table {
        schema: Option<String>,
    },
    Column {
        tables: Vec<TableRef>,
    },
    InsertColumn {
        table: String,
        schema: Option<String>,
        alias: Option<String>,
    },
    UpdateColumn {
        table: String,
        schema: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionContext {
    pub prefix: String,
    pub replace_start: Cursor,
    pub qualifier_parts: Vec<String>,
    pub intent: CompletionIntent,
}

pub fn cursor_offset(sql: &str, cursor: Cursor) -> usize {
    crate::common::utils::cursor::cursor_to_byte_offset(sql, cursor)
}

pub fn offset_to_cursor(sql: &str, offset: usize) -> Cursor {
    crate::common::utils::cursor::byte_offset_to_cursor(sql, offset)
}

pub fn get_completion_context(sql: &str, cursor: Cursor) -> CompletionContext {
    let offset = cursor_offset(sql, cursor);
    let legacy = get_completion_context_heuristic(sql, cursor);
    let model = super::semantic::build_semantic_model(sql, offset);
    super::semantic::completion_context_from_semantic(&model, sql, cursor, legacy)
}

pub(crate) fn get_completion_context_heuristic(sql: &str, cursor: Cursor) -> CompletionContext {
    let offset = cursor_offset(sql, cursor);
    if is_suppressed(sql, offset) {
        return CompletionContext {
            prefix: String::new(),
            replace_start: cursor,
            qualifier_parts: Vec::new(),
            intent: CompletionIntent::Suppressed,
        };
    }

    let stmt_start = statement_start(sql, offset);
    let before = &sql[stmt_start..offset];
    let trailing = parse_trailing_identifier(before);
    let prefix = trailing.prefix.clone();
    let replace_start = offset_to_cursor(sql, stmt_start + trailing.replace_start);
    let qualifier_parts = trailing.qualifier_parts.clone();

    // Resolve referenced tables from the whole statement (not just before the
    // cursor), so column completion inside the select list can use tables that
    // appear in `from` after the cursor (e.g. `select <cursor> from t1`).
    let referenced = extract_referenced_tables(&sql[stmt_start..]);

    if let Some(insert) = detect_insert_column_list(before) {
        return CompletionContext {
            prefix,
            replace_start,
            qualifier_parts,
            intent: CompletionIntent::InsertColumn {
                table: insert.table,
                schema: insert.schema,
                alias: insert.alias,
            },
        };
    }

    if let Some(update) = detect_update_column_context(before) {
        return CompletionContext {
            prefix,
            replace_start,
            qualifier_parts,
            intent: CompletionIntent::UpdateColumn {
                table: update.table,
                schema: update.schema,
            },
        };
    }

    let exclusive_table = is_table_completion_context(before, &referenced) || is_after_table_trigger(before);
    if exclusive_table || is_after_table_trigger(before) {
        let schema = if qualifier_parts.len() == 1 {
            Some(qualifier_parts[0].clone())
        } else {
            None
        };
        return CompletionContext {
            prefix,
            replace_start,
            qualifier_parts,
            intent: CompletionIntent::Table { schema },
        };
    }

    if !qualifier_parts.is_empty() || trailing.ends_with_dot {
        let tables = resolve_qualified_tables(&referenced, &qualifier_parts);
        if !tables.is_empty() {
            return CompletionContext {
                prefix,
                replace_start,
                qualifier_parts,
                intent: CompletionIntent::Column { tables },
            };
        }
    }

    if is_column_context(before) && !referenced.is_empty() {
        return CompletionContext {
            prefix,
            replace_start,
            qualifier_parts,
            intent: CompletionIntent::Column { tables: referenced },
        };
    }

    CompletionContext {
        prefix,
        replace_start,
        qualifier_parts,
        intent: CompletionIntent::Keyword,
    }
}

/// Explicit Shift+Tab after a resolved row source with trailing whitespace → CLAUSE_KEYWORDS subset.
// Kept as part of the complete context API; consumed by the editor integration
// (deferred until the editor buffer is wired).
#[allow(dead_code)]
pub(crate) fn is_typing_clause_keyword_after_row_source(sql: &str, cursor: Cursor) -> bool {
    let offset = cursor_offset(sql, cursor);
    let model = super::semantic::build_semantic_model(sql, offset);
    if matches!(
        model.cursor_intent.kind,
        super::semantic::CursorKind::Keyword
    ) && model.cursor_intent.confidence == super::semantic::Confidence::High
        && !model.cursor_intent.prefix.is_empty()
    {
        return true;
    }

    let stmt_start = statement_start(sql, offset);
    let before = &sql[stmt_start..offset];
    let trailing = parse_trailing_identifier(before);
    if trailing.prefix.is_empty() {
        return false;
    }
    let ident_before = super::ident::byte_prefix(before, trailing.replace_start).trim_end();
    let Some(prev) = last_word(ident_before) else {
        return false;
    };
    let referenced = extract_referenced_tables(before);
    word_is_row_source(&referenced, &prev)
}

pub(crate) fn is_select_list_column_context(sql: &str, cursor: Cursor) -> bool {
    let offset = cursor_offset(sql, cursor);
    let stmt_start = statement_start(sql, offset);
    let before = &sql[stmt_start..offset];
    is_select_list_column_context_before(before)
}

fn is_select_list_column_context_before(before: &str) -> bool {
    let trailing = parse_trailing_identifier(before);
    let stmt_before = super::ident::byte_prefix(before, trailing.replace_start).trim_end();
    let Some(prev) = last_word(stmt_before) else {
        return false;
    };
    if matches!(
        prev.to_ascii_lowercase().as_str(),
        "where" | "on" | "and" | "or" | "having" | "by" | "set"
    ) {
        return false;
    }
    if prev.eq_ignore_ascii_case("select") {
        let lower = stmt_before.to_ascii_lowercase();
        return !lower.contains(" from ");
    }
    let lower = stmt_before.to_ascii_lowercase();
    lower.contains("select ") && !lower.contains(" from ")
}

#[allow(dead_code)]
pub(crate) fn resolve_clause_only_keywords(
    sql: &str,
    cursor: Cursor,
    context: &CompletionContext,
    explicit: bool,
) -> bool {
    if !explicit {
        return false;
    }
    if is_after_row_source_keyword_context(sql, cursor) {
        return true;
    }
    matches!(context.intent, CompletionIntent::Table { .. }) && !context.prefix.is_empty()
}

/// Explicit Shift+Tab after a resolved row source with trailing whitespace → CLAUSE_KEYWORDS subset.
#[allow(dead_code)]
pub(crate) fn is_after_row_source_keyword_context(sql: &str, cursor: Cursor) -> bool {
    let offset = cursor_offset(sql, cursor);
    if is_suppressed(sql, offset) {
        return false;
    }
    let stmt_start = statement_start(sql, offset);
    let before = &sql[stmt_start..offset];
    if !before.ends_with(|c: char| c.is_whitespace()) {
        return false;
    }
    if is_after_table_trigger(before) {
        return false;
    }
    let referenced = extract_referenced_tables(before);
    let trimmed = before.trim_end();
    let Some(table) = last_word(trimmed) else {
        return false;
    };
    word_is_row_source(&referenced, &table)
}

fn ends_with_on_whitespace(before: &str) -> bool {
    if !before.ends_with(|c: char| c.is_whitespace()) {
        return false;
    }
    before.trim_end().to_ascii_lowercase().ends_with("on")
}

/// True when `before` ends in whitespace and the word immediately before the
/// space is a clause keyword that begins a column/expression list (`where`,
/// `on`, `and`, ...). In that position a column-completion popup should
/// auto-open even though the cursor sits after a space. This extends the
/// original dbm's `ends_with_on_whitespace` (which only handled `on`) to the
/// other common clauses, fixing `select * from t where ` not popping columns.
fn ends_with_clause_whitespace(before: &str) -> bool {
    if !before.ends_with(|c: char| c.is_whitespace()) {
        return false;
    }
    matches!(
        last_word(before.trim_end())
            .map(|w| w.to_ascii_lowercase())
            .as_deref(),
        Some(
            "where" | "on" | "and" | "or" | "having" | "using" | "group" | "order" | "by"
                | "qualify"
        )
    )
}

fn is_auto_open_trigger_char(c: char) -> bool {
    super::ident::is_ident_part(c) || matches!(c, '.' | '$' | '@')
}

pub fn should_auto_open(sql: &str, cursor: Cursor) -> bool {
    let offset = cursor_offset(sql, cursor);
    if is_suppressed(sql, offset) {
        return false;
    }
    let before = &sql[..offset];
    let Some(previous_char) = before.chars().last() else {
        return false;
    };

    if ends_with_on_whitespace(before) {
        return true;
    }

    if matches!(previous_char, ',' | ';' | '(' | ')' | '[' | ']') {
        return false;
    }

    let context = get_completion_context(sql, cursor);
    match &context.intent {
        CompletionIntent::Suppressed => false,
        CompletionIntent::Table { .. }
        | CompletionIntent::InsertColumn { .. }
        | CompletionIntent::UpdateColumn { .. } => true,
        CompletionIntent::Column { .. } => {
            !context.qualifier_parts.is_empty()
                || is_auto_open_trigger_char(previous_char)
                || ends_with_clause_whitespace(before)
                // `SELECT <cursor> FROM t`: the cursor sits after a space, but
                // the select column list still needs to auto-open columns.
                || is_select_list_column_context(sql, cursor)
        }
        CompletionIntent::Keyword => is_auto_open_trigger_char(previous_char),
    }
}

#[cfg(test)]
pub fn should_refresh_completion(
    sql: &str,
    cursor: Cursor,
    context: &CompletionContext,
    popup_open: bool,
) -> bool {
    if matches!(context.intent, CompletionIntent::Suppressed) {
        return false;
    }
    popup_open || should_auto_open(sql, cursor)
}

pub fn should_offer_completion(context: &CompletionContext, sql: &str) -> bool {
    should_offer_completion_inner(context, sql, false)
}

pub fn should_offer_completion_explicit(context: &CompletionContext, sql: &str) -> bool {
    should_offer_completion_inner(context, sql, true)
}

fn should_offer_completion_inner(context: &CompletionContext, sql: &str, explicit: bool) -> bool {
    if sql.trim().is_empty() {
        return false;
    }
    match context.intent {
        CompletionIntent::Suppressed => false,
        CompletionIntent::Keyword => explicit || !context.prefix.is_empty(),
        CompletionIntent::Table { .. }
        | CompletionIntent::Column { .. }
        | CompletionIntent::InsertColumn { .. }
        | CompletionIntent::UpdateColumn { .. } => true,
    }
}

struct TrailingIdentifier {
    prefix: String,
    qualifier_parts: Vec<String>,
    replace_start: usize,
    ends_with_dot: bool,
}

struct InsertTarget {
    table: String,
    schema: Option<String>,
    alias: Option<String>,
}

struct UpdateTarget {
    table: String,
    schema: Option<String>,
}

pub(crate) fn stmt_before_trailing_ident(sql: &str, cursor: Cursor) -> String {
    let offset = cursor_offset(sql, cursor);
    let stmt_start = statement_start(sql, offset);
    let before = &sql[stmt_start..offset];
    let trailing = parse_trailing_identifier(before);
    super::ident::byte_prefix(before, trailing.replace_start)
        .trim_end()
        .to_string()
}

fn parse_trailing_identifier(before: &str) -> TrailingIdentifier {
    if before.is_empty() || before.ends_with(|c: char| c.is_whitespace()) {
        let ends_with_dot = before.trim_end().ends_with('.');
        return TrailingIdentifier {
            prefix: String::new(),
            qualifier_parts: Vec::new(),
            replace_start: before.len(),
            ends_with_dot,
        };
    }

    let (start, scan_end, ends_with_dot) = super::ident::scan_trailing_ident_range(before);
    let token = &before[start..scan_end];
    let parts: Vec<String> = token
        .split('.')
        .map(super::ident::unquote_ident)
        .filter(|p| !p.is_empty())
        .collect();

    if ends_with_dot || parts.len() > 1 {
        let qualifier_parts = if ends_with_dot {
            parts.clone()
        } else {
            parts[..parts.len().saturating_sub(1)].to_vec()
        };
        let prefix = if ends_with_dot {
            String::new()
        } else {
            parts.last().cloned().unwrap_or_default()
        };
        TrailingIdentifier {
            prefix,
            qualifier_parts,
            replace_start: start,
            ends_with_dot,
        }
    } else {
        TrailingIdentifier {
            prefix: parts.first().cloned().unwrap_or_default(),
            qualifier_parts: Vec::new(),
            replace_start: start,
            ends_with_dot: false,
        }
    }
}

fn last_word(before: &str) -> Option<String> {
    let trimmed = before.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    let (start, end, _) = super::ident::scan_trailing_ident_range(trimmed);
    if start >= end {
        return None;
    }
    let token = trimmed[start..end].trim();
    if token.is_empty() {
        return None;
    }
    let last_segment = token.rsplit('.').next().unwrap_or(token);
    let name = super::ident::unquote_ident(last_segment);
    if name.is_empty() { None } else { Some(name) }
}

fn statement_start(sql: &str, offset: usize) -> usize {
    super::ident::statement_start_before(sql, offset)
}

fn is_suppressed(sql: &str, offset: usize) -> bool {
    let offset = super::ident::snap_to_char_boundary(sql, offset);
    let mut in_single = false;
    let mut in_double = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut chars = sql.char_indices().peekable();
    while let Some((byte_idx, ch)) = chars.next() {
        if byte_idx >= offset {
            break;
        }
        let next_ch = chars.peek().map(|(_, c)| *c);
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
            }
            continue;
        }
        if in_block_comment {
            if ch == '*' && next_ch == Some('/') {
                chars.next();
                in_block_comment = false;
            }
            continue;
        }
        if in_single {
            if ch == '\'' {
                if next_ch == Some('\'') {
                    chars.next();
                    continue;
                }
                in_single = false;
            }
            continue;
        }
        if in_double {
            if ch == '"' {
                if next_ch == Some('"') {
                    chars.next();
                    continue;
                }
                in_double = false;
            }
            continue;
        }
        if ch == '-' && next_ch == Some('-') {
            in_line_comment = true;
            chars.next();
            continue;
        }
        if ch == '/' && next_ch == Some('*') {
            in_block_comment = true;
            chars.next();
            continue;
        }
        if ch == '\'' {
            in_single = true;
            continue;
        }
        if ch == '"' {
            in_double = true;
        }
    }
    in_single || in_double || in_line_comment || in_block_comment
}

fn strip_single_quoted_literals(sql: &str) -> String {
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
        } else {
            out.push(ch);
        }
    }
    out
}

/// `INSERT INTO table ( col1, col2` — cursor still before the closing `)`.
pub(crate) fn is_inside_insert_column_list(before: &str) -> bool {
    insert_column_list_qualified(before).is_some()
}

/// `INSERT INTO table (cols)` closed — next token should be `VALUES`.
pub(crate) fn is_after_insert_column_list(before: &str) -> bool {
    let lower = before.to_ascii_lowercase();
    if !lower.contains("insert into ") {
        return false;
    }
    if lower.contains(" values") {
        return false;
    }
    if is_inside_insert_column_list(before) {
        return false;
    }
    let trimmed = before.trim_end();
    if !trimmed.ends_with(')') {
        return false;
    }
    let Some(idx) = lower.rfind("insert into ") else {
        return false;
    };
    before[idx + "insert into ".len()..].contains('(')
}

fn insert_column_list_qualified(before: &str) -> Option<String> {
    let cleaned = strip_single_quoted_literals(before);
    let lower = cleaned.to_ascii_lowercase();
    let idx = lower.rfind("insert into ")?;
    let rest = &cleaned[idx + "insert into ".len()..];
    let open_paren = rest.find('(')?;
    let qualified = rest[..open_paren].trim();
    if qualified.is_empty() || qualified.to_ascii_lowercase().contains(" values") {
        return None;
    }
    let after_qualified = rest[open_paren..].trim_start();
    if !after_qualified.starts_with('(') {
        return None;
    }
    let after_open = &rest[open_paren + 1..];
    if after_open.contains(')') {
        return None;
    }
    Some(qualified.to_string())
}

fn detect_insert_column_list(before: &str) -> Option<InsertTarget> {
    let qualified = insert_column_list_qualified(before)?;
    Some(parse_insert_target(&qualified))
}

fn parse_insert_target(qualified: &str) -> InsertTarget {
    let qualified = qualified.trim();
    if qualified.is_empty() {
        return InsertTarget {
            table: String::new(),
            schema: None,
            alias: None,
        };
    }
    let ident_end = super::ident::scan_ident_end(qualified);
    if ident_end == 0 {
        return InsertTarget {
            table: qualified.to_string(),
            schema: None,
            alias: None,
        };
    }
    let parsed = parse_qualified_table(&qualified[..ident_end]);
    let alias = read_table_alias(&qualified[ident_end..]);
    InsertTarget {
        table: parsed.table,
        schema: parsed.schema,
        alias,
    }
}

fn detect_update_column_context(before: &str) -> Option<UpdateTarget> {
    let lower = before.to_ascii_lowercase();
    let idx = lower.rfind("update ")?;
    let rest = &before[idx + "update ".len()..];
    let lower_rest = rest.to_ascii_lowercase();
    let set_idx = lower_rest.find(" set")?;
    let qualified = rest[..set_idx].trim();
    if qualified.is_empty() {
        return None;
    }
    let target = parse_qualified_table(qualified);
    Some(UpdateTarget {
        table: target.table,
        schema: target.schema,
    })
}

fn parse_qualified_table(qualified: &str) -> InsertTarget {
    let parts: Vec<String> = qualified
        .split('.')
        .map(super::ident::unquote_ident)
        .filter(|p| !p.is_empty())
        .collect();
    match parts.as_slice() {
        [table] => InsertTarget {
            table: table.clone(),
            schema: None,
            alias: None,
        },
        [schema, table] => InsertTarget {
            table: table.clone(),
            schema: Some(schema.clone()),
            alias: None,
        },
        parts if parts.len() >= 2 => InsertTarget {
            table: parts.last().cloned().unwrap_or_default(),
            schema: Some(parts[parts.len() - 2].clone()),
            alias: None,
        },
        _ => InsertTarget {
            table: qualified.to_string(),
            schema: None,
            alias: None,
        },
    }
}

fn is_after_table_trigger(before: &str) -> bool {
    let trimmed = before.trim_end();
    trimmed.ends_with(',') && trimmed.to_ascii_lowercase().contains("from")
}

fn is_table_completion_context(before: &str, referenced: &[TableRef]) -> bool {
    const INTRODUCERS: &[&str] = &["from", "join", "into", "update", "table"];
    const JOIN_MODS: &[&str] = &[
        "left", "right", "inner", "outer", "cross", "full", "natural",
    ];

    let trailing = parse_trailing_identifier(before);
    let stmt_before = super::ident::byte_prefix(before, trailing.replace_start).trim_end();

    // Multi-table lists use commas (FROM a, b). Space after a resolved name starts a clause.
    if before.ends_with(|c: char| c.is_whitespace()) {
        if stmt_before.ends_with(',') {
            return true;
        }
        if let Some(prev) = last_word(stmt_before) {
            if word_is_row_source(referenced, &prev) {
                return false;
            }
            let prev_lower = prev.to_ascii_lowercase();
            if INTRODUCERS.contains(&prev_lower.as_str()) {
                return true;
            }
        }
        return false;
    }

    if trailing.prefix.is_empty() {
        return false;
    }

    let ident_before = super::ident::byte_prefix(before, trailing.replace_start).trim_end();
    let Some(prev) = last_word(ident_before) else {
        return false;
    };
    if word_is_row_source(referenced, &prev) && !trailing.prefix.eq_ignore_ascii_case(&prev) {
        return false;
    }
    let prev_lower = prev.to_ascii_lowercase();

    if INTRODUCERS.contains(&prev_lower.as_str()) || JOIN_MODS.contains(&prev_lower.as_str()) {
        return true;
    }
    if ident_before.ends_with(',') {
        return true;
    }

    false
}

fn word_is_row_source(referenced: &[TableRef], word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    referenced.iter().any(|t| {
        t.name.eq_ignore_ascii_case(word)
            || t.alias
                .as_ref()
                .is_some_and(|a| a.eq_ignore_ascii_case(word))
    })
}

fn is_column_context(before: &str) -> bool {
    let trailing = parse_trailing_identifier(before);
    let stmt_before = super::ident::byte_prefix(before, trailing.replace_start).trim_end();
    let Some(prev) = last_word(stmt_before) else {
        return false;
    };
    let prev = prev.to_ascii_lowercase();
    if matches!(
        prev.as_str(),
        "where" | "on" | "and" | "or" | "having" | "by" | "set"
    ) {
        return true;
    }
    let lower = stmt_before.to_ascii_lowercase();
    // Any position inside a `SELECT <cols>` list (before `from`) is a column
    // context — even right after a typed column name (`select t ` or
    // `select t`), so the column popup keeps offering.
    if lower.contains("select ") && !lower.contains(" from ") {
        return true;
    }
    false
}

pub(crate) fn extract_referenced_tables(stmt: &str) -> Vec<TableRef> {
    let lower = stmt.to_ascii_lowercase();
    let mut tables = Vec::new();
    // `from`/`join`/`into`/`update` introduce a referenced table. `into` and
    // `update` are needed so column completion for `INSERT ... (col` and
    // `UPDATE t SET` resolves the target table's columns.
    for keyword in ["from ", "join ", "into ", "update "] {
        let mut search_from = 0usize;
        while search_from < lower.len() {
            while search_from < lower.len() && !lower.is_char_boundary(search_from) {
                search_from = super::ident::advance_byte_offset(&lower, search_from);
            }
            let Some(rel) = lower[search_from..].find(keyword) else {
                break;
            };
            let start = search_from + rel + keyword.len();
            if start >= stmt.len() {
                break;
            }
            let tail = &stmt[start..];
            let trim_skip = tail.len() - tail.trim_start().len();
            let ident_end = super::ident::scan_ident_end(tail);
            if ident_end <= trim_skip {
                search_from = super::ident::advance_byte_offset(stmt, start);
                continue;
            }
            let qualified = tail[trim_skip..ident_end].trim();
            if qualified.is_empty() {
                search_from = super::ident::advance_byte_offset(stmt, start);
                continue;
            }
            let parsed = parse_qualified_table(qualified);
            tables.push(TableRef {
                name: parsed.table,
                schema: parsed.schema,
                alias: read_table_alias(&tail[ident_end..]),
            });
            search_from = start + ident_end;
        }
    }
    tables
}

fn read_table_alias(rest: &str) -> Option<String> {
    let rest = rest.trim_start();
    let lower_rest = rest.to_ascii_lowercase();
    if lower_rest.starts_with("as ") {
        return rest[3..]
            .split_whitespace()
            .next()
            .map(super::ident::unquote_ident)
            .and_then(|word| sanitize_table_alias(&word));
    }
    let word = rest.split_whitespace().next()?;
    sanitize_table_alias(&super::ident::unquote_ident(word))
}

fn resolve_qualified_tables(referenced: &[TableRef], qualifier_parts: &[String]) -> Vec<TableRef> {
    if qualifier_parts.is_empty() {
        return referenced.to_vec();
    }
    if qualifier_parts.len() == 1 {
        let q = qualifier_parts[0].to_ascii_lowercase();
        let matches: Vec<TableRef> = referenced
            .iter()
            .filter(|t| {
                t.alias.as_ref().is_some_and(|a| a.eq_ignore_ascii_case(&q))
                    || t.name.eq_ignore_ascii_case(&q)
            })
            .cloned()
            .collect();
        if !matches.is_empty() {
            return matches;
        }
        return vec![TableRef {
            name: qualifier_parts[0].clone(),
            schema: None,
            alias: None,
        }];
    }
    if qualifier_parts.len() >= 2 {
        let schema = qualifier_parts[qualifier_parts.len() - 2].clone();
        let name = qualifier_parts.last().cloned().unwrap_or_default();
        return vec![TableRef {
            name,
            schema: Some(schema),
            alias: None,
        }];
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor_at(sql: &str) -> Cursor {
        Cursor::new(0, sql.chars().count())
    }

    #[test]
    fn extract_referenced_tables_includes_mutation_targets() {
        // `update`/`into` targets must count as referenced tables so column
        // completion for `UPDATE t SET` / `INSERT INTO t (` can resolve columns.
        let refs = extract_referenced_tables("update tb1 set name = ");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "tb1");
        assert!(refs[0].alias.is_none());

        let refs = extract_referenced_tables("insert into tb2 (id, na");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "tb2");
    }

    #[test]
    fn extract_referenced_tables_ignores_clause_typing_as_alias() {
        let refs = extract_referenced_tables("select * from users w");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "users");
        assert!(refs[0].alias.is_none(), "w must not become alias");

        let refs = extract_referenced_tables("select * from users wh");
        assert_eq!(refs.len(), 1);
        assert!(refs[0].alias.is_none());

        let refs = extract_referenced_tables("select * from users us");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].alias.as_deref(), Some("us"));
    }

    #[test]
    fn after_insert_column_list_is_keyword_not_columns() {
        let sql = "INSERT INTO \"测试表\" (id, \"名称\", \"描述\") ";
        let ctx = get_completion_context(sql, cursor_at(sql));
        assert!(
            matches!(ctx.intent, CompletionIntent::Keyword),
            "got {:?}",
            ctx.intent
        );
        assert!(is_after_insert_column_list(
            &sql[..cursor_offset(sql, cursor_at(sql))]
        ));
    }

    #[test]
    fn typing_v_after_insert_column_list_offers_values() {
        let sql = "insert into users (id, c1) v";
        let ctx = get_completion_context(sql, cursor_at(sql));
        assert!(matches!(ctx.intent, CompletionIntent::Keyword));
        assert_eq!(ctx.prefix, "v");
    }

    #[test]
    fn inside_insert_column_list_still_column_intent() {
        let sql = "insert into users (id, na";
        let ctx = get_completion_context(sql, cursor_at(sql));
        assert!(matches!(ctx.intent, CompletionIntent::InsertColumn { .. }));
    }

    #[test]
    fn insert_column_list_parses_table_alias() {
        let sql = "insert into 测试表 as tb (";
        let ctx = get_completion_context(sql, cursor_at(sql));
        assert!(matches!(ctx.intent, CompletionIntent::InsertColumn { .. }));
        if let CompletionIntent::InsertColumn { table, alias, .. } = ctx.intent {
            assert_eq!(table, "测试表");
            assert_eq!(alias.as_deref(), Some("tb"));
        }
    }

    #[test]
    fn alias_column_uses_semantic_sources() {
        let sql = "SELECT u. FROM users u";
        let cursor_pos = "SELECT u.".chars().count();
        let ctx = get_completion_context(sql, Cursor::new(0, cursor_pos));
        assert!(matches!(ctx.intent, CompletionIntent::Column { .. }));
        if let CompletionIntent::Column { tables } = ctx.intent {
            assert!(tables.iter().any(|t| t.name == "users"));
        }
    }

    #[test]
    fn table_trigger_after_from() {
        let sql = "SELECT * FROM ord";
        let ctx = get_completion_context(sql, cursor_at(sql));
        assert!(matches!(ctx.intent, CompletionIntent::Table { .. }));
    }

    #[test]
    fn suppresses_in_single_quote() {
        let sql = "SELECT 'foo";
        let ctx = get_completion_context(sql, cursor_at(sql));
        assert!(matches!(ctx.intent, CompletionIntent::Suppressed));
    }

    #[test]
    fn chinese_table_trailing_space_is_keyword_not_table() {
        let sql = "select * from 测试表 ";
        let ctx = get_completion_context(sql, cursor_at(sql));
        assert!(
            matches!(ctx.intent, CompletionIntent::Keyword),
            "got {:?}",
            ctx.intent
        );
        assert!(should_offer_completion_explicit(&ctx, sql));
    }

    #[test]
    fn last_word_handles_quoted_unicode_table() {
        assert_eq!(
            last_word("select * from \"测试表\""),
            Some("测试表".to_string())
        );
        assert_eq!(
            last_word("select * from 测试表"),
            Some("测试表".to_string())
        );
        assert_eq!(
            last_word("select * from public.\"测试表\""),
            Some("测试表".to_string())
        );
    }

    #[test]
    fn after_row_source_keyword_context_after_quoted_chinese_table() {
        let sql = "select * from \"测试表\" ";
        let cursor = cursor_at(sql);
        let refs = extract_referenced_tables(sql);
        assert_eq!(refs.len(), 1, "refs: {refs:?}");
        assert_eq!(refs[0].name, "测试表");
        assert!(is_after_row_source_keyword_context(sql, cursor));
    }

    #[test]
    fn quoted_chinese_table_w_is_keyword() {
        let sql = "select * from \"测试表\" w";
        let ctx = get_completion_context(sql, cursor_at(sql));
        assert!(
            matches!(ctx.intent, CompletionIntent::Keyword),
            "got {:?}",
            ctx.intent
        );
        assert_eq!(ctx.prefix, "w");
        assert!(should_auto_open(sql, cursor_at(sql)));
    }

    #[test]
    fn from_users_trailing_space_is_keyword_not_table() {
        let sql = "SELECT * FROM users ";
        let cursor = cursor_at(sql);
        let ctx = get_completion_context(sql, cursor);
        assert!(
            matches!(ctx.intent, CompletionIntent::Keyword),
            "expected Keyword, got {:?}",
            ctx.intent
        );
        assert!(should_offer_completion_explicit(&ctx, sql));
        assert!(!should_offer_completion(&ctx, sql));
    }

    #[test]
    fn from_comma_continues_table_list() {
        let sql = "SELECT * FROM a, ";
        let ctx = get_completion_context(sql, cursor_at(sql));
        assert!(matches!(ctx.intent, CompletionIntent::Table { .. }));
    }

    #[test]
    fn from_table_then_partial_where_is_keyword_not_column() {
        let sql = "select * from users wh";
        let ctx = get_completion_context(sql, cursor_at(sql));
        assert!(matches!(ctx.intent, CompletionIntent::Keyword));
    }

    #[test]
    fn should_auto_open_after_from_space() {
        let sql = "select * from ";
        assert!(should_auto_open(sql, cursor_at(sql)));
    }

    #[test]
    fn should_not_offer_keyword_completion_without_prefix() {
        let sql = "select ";
        let ctx = get_completion_context(sql, cursor_at(sql));
        assert!(!should_offer_completion(&ctx, sql));
    }

    #[test]
    fn should_not_auto_open_on_empty_sql() {
        assert!(!should_auto_open("", Cursor::new(0, 0)));
    }

    #[test]
    fn should_not_auto_open_after_table_trailing_space() {
        let sql = "SELECT * FROM users ";
        assert!(!should_auto_open(sql, cursor_at(sql)));
    }

    #[test]
    fn should_not_auto_open_on_structural_punctuation() {
        for sql in [
            "select count(*)",
            "select * from users;",
            "select * from users,",
        ] {
            assert!(
                !should_auto_open(sql, cursor_at(sql)),
                "{sql}"
            );
        }
    }

    #[test]
    fn should_auto_open_on_join_on_whitespace() {
        let sql = "select * from public.users u join public.orders o on ";
        assert!(should_auto_open(sql, cursor_at(sql)));
    }

    #[test]
    fn should_auto_open_after_where_whitespace() {
        // `where ` (a clause that begins a column/expression list) must
        // auto-open the column popup even though the cursor follows a space.
        let sql = "select * from tb1 where ";
        assert!(
            should_auto_open(sql, cursor_at(sql)),
            "`where ` must auto-open a column popup"
        );
    }

    #[test]
    fn should_auto_open_after_and_or_having_whitespace() {
        for clause in ["and ", "or ", "having ", "using "] {
            let sql = format!("select * from tb1 where id = 1 {clause}");
            assert!(
                should_auto_open(&sql, cursor_at(&sql)),
                "`{clause}` must auto-open a column popup"
            );
        }
    }

    #[test]
    fn should_not_auto_open_bare_identifier_space() {
        // A space after a non-clause word (e.g. an alias or a value) must NOT
        // auto-open — only the recognized clause keywords do.
        let sql = "select * from tb1 a ";
        assert!(
            !should_auto_open(sql, cursor_at(sql)),
            "`a ` (a plain alias) must not auto-open"
        );
    }

    #[test]
    fn should_offer_completion_after_select_space() {
        // `SELECT ` starts a column list, so it must offer column completion
        // even though the cursor follows a space. This is the gate the original
        // dbm uses (build_completion_state_inner → should_offer_completion),
        // fixing `SELECT <cursor> FROM t` not popping the column list.
        let sql = "SELECT  FROM tb1";
        let cursor = cursor_at("SELECT ");
        let context = get_completion_context(sql, cursor);
        assert!(
            matches!(context.intent, CompletionIntent::Column { .. }),
            "expected Column intent, got {:?}",
            context.intent
        );
        assert!(
            should_offer_completion(&context, sql),
            "`SELECT ` must offer column completion"
        );
    }

    #[test]
    fn utf8_smoke_cases_do_not_panic() {
        let cases = [
            "select * from 测",
            "select * from \"测试\"",
            "select 测 from 表",
            "insert into 测试表 (字",
            "update 测试表 set 字",
            "'中文'",
            "select '中文' from ",
        ];
        for sql in cases {
            for col in 0..=sql.chars().count() {
                let cursor = Cursor::new(0, col);
                let _ = get_completion_context(sql, cursor);
                let _ = should_auto_open(sql, cursor);
            }
        }
    }
}
