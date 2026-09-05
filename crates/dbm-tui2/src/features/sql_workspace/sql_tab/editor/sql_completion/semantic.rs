//! Semantic SQL analysis for completion (DBX-aligned).
//!
//! Builds a token-based [`SemanticModel`] of the statement at the cursor and
//! merges its high-confidence intent into the heuristic [`CompletionContext`].

use crate::common::utils::cursor::Cursor;

use super::alias_blacklist::{is_alias_blacklisted, is_clause_typing_prefix};
use super::context::{CompletionContext, CompletionIntent, TableRef};
use super::ident;
use super::tokens::{self, SemanticToken, TokenKind};

const TABLE_INTRODUCERS: &[&str] = &["from", "join", "update", "into", "using"];
const JOIN_MODIFIERS: &[&str] = &[
    "left", "right", "inner", "outer", "cross", "full", "natural",
];
const CLAUSE_WORDS: &[&str] = &[
    "where",
    "group",
    "having",
    "order",
    "limit",
    "offset",
    "union",
    "on",
    "set",
    "values",
    "returning",
    "select",
    "from",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    Select,
    Insert,
    Update,
    Delete,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorKind {
    Suppressed,
    Table,
    Column,
    AliasColumn,
    InsertColumn,
    UpdateColumn,
    Keyword,
}

#[derive(Debug, Clone)]
pub struct RowSource {
    pub name: String,
    pub schema: Option<String>,
    pub alias: Option<String>,
    pub is_mutation_target: bool,
}

#[derive(Debug, Clone)]
pub struct CursorIntent {
    pub kind: CursorKind,
    pub prefix: String,
    pub replacement_start: usize,
    pub qualifier_parts: Vec<String>,
    pub confidence: Confidence,
    pub target_source: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct SemanticModel {
    #[allow(dead_code)]
    pub statement_kind: StatementKind,
    pub row_sources: Vec<RowSource>,
    pub cursor_intent: CursorIntent,
}

pub fn build_semantic_model(sql: &str, cursor: usize) -> SemanticModel {
    let safe_cursor = cursor.min(sql.len());
    let all_tokens = tokens::tokenize_sql(sql);
    let stmt_start = statement_start(sql, safe_cursor);
    let stmt_end = sql[stmt_start..]
        .find(';')
        .map(|idx| stmt_start + idx)
        .unwrap_or(sql.len());
    let stmt_tokens: Vec<SemanticToken> = all_tokens
        .iter()
        .filter(|t| t.span.end > stmt_start && t.span.start < stmt_end)
        .cloned()
        .collect();
    let cursor_tokens: Vec<SemanticToken> = stmt_tokens
        .iter()
        .filter(|t| t.span.start < safe_cursor)
        .cloned()
        .collect();
    let significant = tokens::significant_tokens(&stmt_tokens);
    let statement_kind = statement_kind(&significant);
    let suppressed = tokens::is_suppressed_at_cursor(&all_tokens, safe_cursor);
    let row_sources = parse_row_sources(&stmt_tokens, statement_kind);
    let cursor_intent = build_cursor_intent(
        &cursor_tokens,
        safe_cursor,
        &row_sources,
        suppressed,
        statement_kind,
        sql,
    );
    SemanticModel {
        statement_kind,
        row_sources,
        cursor_intent,
    }
}

pub fn completion_context_from_semantic(
    model: &SemanticModel,
    sql: &str,
    cursor: Cursor,
    legacy: CompletionContext,
) -> CompletionContext {
    if model.cursor_intent.kind == CursorKind::Suppressed {
        return legacy;
    }
    if matches!(legacy.intent, CompletionIntent::Suppressed) {
        return legacy;
    }
    // A select column list always offers columns: don't let a semantic Keyword
    // intent override the heuristic Column intent here (e.g. after `select t `
    // the cursor sits right after the column name). This keeps the column popup
    // open instead of collapsing to keywords.
    if matches!(legacy.intent, CompletionIntent::Column { .. })
        && super::context::is_select_list_column_context(sql, cursor)
    {
        return legacy;
    }

    let prefer_semantic = should_prefer_semantic(model, &legacy);
    if !prefer_semantic {
        return legacy;
    }

    let intent = map_cursor_intent(model);
    // Keep the heuristic's replace_start: it correctly covers a typed qualifier
    // (e.g. `select t.id, t.` → replace_start at the second `t`), whereas the
    // semantic model can point past it, causing a duplicated prefix on apply
    // (`t.t.name`). The prefix/qualifier still come from the semantic model.
    CompletionContext {
        prefix: model.cursor_intent.prefix.clone(),
        replace_start: legacy.replace_start,
        qualifier_parts: model.cursor_intent.qualifier_parts.clone(),
        intent,
    }
}

fn should_prefer_semantic(model: &SemanticModel, legacy: &CompletionContext) -> bool {
    if matches!(model.cursor_intent.kind, CursorKind::Keyword)
        && matches!(
            legacy.intent,
            CompletionIntent::Table { .. } | CompletionIntent::Column { .. }
        )
    {
        return true;
    }
    match model.cursor_intent.confidence {
        Confidence::High | Confidence::Medium => true,
        Confidence::Low => matches!(model.cursor_intent.kind, CursorKind::Keyword),
    }
}

fn map_cursor_intent(model: &SemanticModel) -> CompletionIntent {
    let sources = &model.row_sources;
    match model.cursor_intent.kind {
        CursorKind::Suppressed => CompletionIntent::Suppressed,
        CursorKind::Table => {
            let schema = model
                .cursor_intent
                .qualifier_parts
                .first()
                .cloned()
                .filter(|_| model.cursor_intent.qualifier_parts.len() == 1);
            CompletionIntent::Table { schema }
        }
        CursorKind::InsertColumn => {
            let target = model
                .cursor_intent
                .target_source
                .and_then(|idx| sources.get(idx))
                .or_else(|| sources.iter().find(|s| s.is_mutation_target));
            if let Some(target) = target {
                CompletionIntent::InsertColumn {
                    table: target.name.clone(),
                    schema: target.schema.clone(),
                    alias: target.alias.clone(),
                }
            } else {
                CompletionIntent::Keyword
            }
        }
        CursorKind::UpdateColumn => {
            let target = model
                .cursor_intent
                .target_source
                .and_then(|idx| sources.get(idx))
                .or_else(|| sources.iter().find(|s| s.is_mutation_target))
                .or_else(|| sources.first());
            if let Some(target) = target {
                CompletionIntent::UpdateColumn {
                    table: target.name.clone(),
                    schema: target.schema.clone(),
                }
            } else {
                CompletionIntent::Column {
                    tables: sources_to_refs(sources),
                }
            }
        }
        CursorKind::AliasColumn | CursorKind::Column => CompletionIntent::Column {
            tables: resolve_column_tables(sources, &model.cursor_intent.qualifier_parts),
        },
        CursorKind::Keyword => CompletionIntent::Keyword,
    }
}

fn sources_to_refs(sources: &[RowSource]) -> Vec<TableRef> {
    sources
        .iter()
        .map(|s| TableRef {
            name: s.name.clone(),
            schema: s.schema.clone(),
            alias: s.alias.clone(),
        })
        .collect()
}

fn resolve_column_tables(sources: &[RowSource], qualifier_parts: &[String]) -> Vec<TableRef> {
    if qualifier_parts.is_empty() {
        return sources_to_refs(sources);
    }
    let q = qualifier_parts[0].to_ascii_lowercase();
    let matched: Vec<TableRef> = sources
        .iter()
        .filter(|s| {
            s.alias.as_ref().is_some_and(|a| a.eq_ignore_ascii_case(&q))
                || s.name.eq_ignore_ascii_case(&q)
        })
        .map(|s| TableRef {
            name: s.name.clone(),
            schema: s.schema.clone(),
            alias: s.alias.clone(),
        })
        .collect();
    if !matched.is_empty() {
        return matched;
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
    vec![TableRef {
        name: qualifier_parts[0].clone(),
        schema: None,
        alias: None,
    }]
}

fn statement_start(sql: &str, cursor: usize) -> usize {
    super::ident::statement_start_before(sql, cursor)
}

fn statement_kind(tokens: &[&SemanticToken]) -> StatementKind {
    let first = tokens
        .iter()
        .find(|t| t.kind == TokenKind::Word)
        .map(|t| t.normalized.as_str());
    match first {
        Some("with") | Some("select") => StatementKind::Select,
        Some("insert") => StatementKind::Insert,
        Some("update") => StatementKind::Update,
        Some("delete") => StatementKind::Delete,
        _ => StatementKind::Unknown,
    }
}

fn parse_row_sources(tokens: &[SemanticToken], statement_kind: StatementKind) -> Vec<RowSource> {
    let root_depth = tokens.first().map(|t| t.depth).unwrap_or(0);
    let mut sources = Vec::new();
    let mut index = 0usize;
    while index < tokens.len() {
        let token = &tokens[index];
        if token.kind != TokenKind::Word || token.depth != root_depth {
            index += 1;
            continue;
        }
        if !TABLE_INTRODUCERS.contains(&token.normalized.as_str()) {
            index += 1;
            continue;
        }
        let introducer = token.normalized.as_str();
        let mut target = index + 1;
        while target < tokens.len() && JOIN_MODIFIERS.contains(&tokens[target].normalized.as_str())
        {
            target += 1;
        }
        if let Some((source, next)) = parse_table_at(tokens, target, introducer, statement_kind) {
            sources.push(source);
            index = next;
        } else {
            index += 1;
        }
    }
    dedupe_sources(sources)
}

fn parse_table_at(
    tokens: &[SemanticToken],
    index: usize,
    introducer: &str,
    statement_kind: StatementKind,
) -> Option<(RowSource, usize)> {
    let (name, schema, next) = read_qualified_name(tokens, index)?;
    let alias = read_alias(tokens, next);
    let is_mutation_target = introducer == "into"
        || introducer == "update"
        || (statement_kind == StatementKind::Delete && introducer == "from");
    Some((
        RowSource {
            name,
            schema,
            alias: alias.0,
            is_mutation_target,
        },
        alias.1,
    ))
}

fn read_qualified_name(
    tokens: &[SemanticToken],
    start: usize,
) -> Option<(String, Option<String>, usize)> {
    let token = tokens.get(start)?;
    if !is_identifier(token) {
        return None;
    }
    let mut parts = vec![ident_name(token)];
    let mut index = start + 1;
    while tokens.get(index).is_some_and(|next| next.text == ".") {
        let next = tokens.get(index + 1)?;
        if !is_identifier(next) {
            break;
        }
        parts.push(ident_name(next));
        index += 2;
    }
    let name = parts.pop()?;
    let schema = parts.pop();
    Some((name, schema, index))
}

fn read_alias(tokens: &[SemanticToken], start: usize) -> (Option<String>, usize) {
    let mut index = start;
    if tokens.get(index).is_some_and(|t| t.normalized == "as") {
        index += 1;
    }
    let Some(token) = tokens.get(index) else {
        return (None, start);
    };
    if !is_identifier(token) {
        return (None, start);
    }
    let name = ident_name(token);
    if is_alias_blacklisted(&name) || is_clause_typing_prefix(&name) {
        return (None, start);
    }
    (Some(name), index + 1)
}

fn dedupe_sources(sources: Vec<RowSource>) -> Vec<RowSource> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for source in sources {
        let key = format!(
            "{}:{}:{}",
            source.name,
            source.schema.as_deref().unwrap_or(""),
            source.alias.as_deref().unwrap_or("")
        );
        if seen.insert(key) {
            out.push(source);
        }
    }
    out
}

struct TrailingIdentifier {
    prefix: String,
    replacement_start: usize,
    qualifier_parts: Vec<String>,
    ends_with_dot: bool,
}

fn build_cursor_intent(
    tokens: &[SemanticToken],
    cursor: usize,
    sources: &[RowSource],
    suppressed: bool,
    statement_kind: StatementKind,
    sql: &str,
) -> CursorIntent {
    if suppressed {
        return CursorIntent {
            kind: CursorKind::Suppressed,
            prefix: String::new(),
            replacement_start: cursor,
            qualifier_parts: Vec::new(),
            confidence: Confidence::High,
            target_source: None,
        };
    }

    let trailing = trailing_identifier(tokens, cursor);
    let previous = previous_word(tokens, cursor);
    let word_before_replacement = word_before(tokens, trailing.replacement_start);
    let target_source = source_for_qualifier(sources, &trailing.qualifier_parts);

    if statement_kind == StatementKind::Insert
        && has_word_before(tokens, cursor, "into")
        && !has_word_before(tokens, cursor, "values")
        && (super::context::is_inside_insert_column_list(
            &sql[ident::statement_start_before(sql, cursor)..cursor],
        ) || trailing.ends_with_dot)
    {
        let idx = sources.iter().position(|s| s.is_mutation_target);
        return CursorIntent {
            kind: CursorKind::InsertColumn,
            prefix: trailing.prefix,
            replacement_start: trailing.replacement_start,
            qualifier_parts: trailing.qualifier_parts,
            confidence: if idx.is_some() {
                Confidence::Medium
            } else {
                Confidence::Low
            },
            target_source: idx,
        };
    }

    if !trailing.qualifier_parts.is_empty() && target_source.is_some() {
        let idx = target_source;
        return CursorIntent {
            kind: CursorKind::AliasColumn,
            prefix: trailing.prefix,
            replacement_start: trailing.replacement_start,
            qualifier_parts: trailing.qualifier_parts,
            confidence: Confidence::High,
            target_source: idx,
        };
    }

    if trailing.ends_with_dot && !trailing.qualifier_parts.is_empty() {
        return CursorIntent {
            kind: CursorKind::AliasColumn,
            prefix: trailing.prefix,
            replacement_start: trailing.replacement_start,
            qualifier_parts: trailing.qualifier_parts.clone(),
            confidence: if target_source.is_some()
                || sources.iter().any(|s| {
                    trailing.qualifier_parts.iter().any(|q| {
                        s.alias.as_ref().is_some_and(|a| a.eq_ignore_ascii_case(q))
                            || s.name.eq_ignore_ascii_case(q)
                    })
                }) {
                Confidence::High
            } else {
                Confidence::Medium
            },
            target_source: source_for_qualifier(sources, &trailing.qualifier_parts),
        };
    }

    // Typing a clause keyword after a resolved table/alias (e.g. users w → WHERE).
    if is_row_source_word(sources, &word_before_replacement)
        && !is_table_list_continuation(tokens, trailing.replacement_start)
    {
        return CursorIntent {
            kind: CursorKind::Keyword,
            prefix: trailing.prefix,
            replacement_start: trailing.replacement_start,
            qualifier_parts: trailing.qualifier_parts,
            confidence: Confidence::High,
            target_source: None,
        };
    }

    // Whitespace after a resolved table name — clause keyword next (not another table).
    if is_clause_after_row_source(tokens, cursor, sources) {
        return CursorIntent {
            kind: CursorKind::Keyword,
            prefix: trailing.prefix,
            replacement_start: trailing.replacement_start,
            qualifier_parts: trailing.qualifier_parts,
            confidence: Confidence::High,
            target_source: None,
        };
    }

    if TABLE_INTRODUCERS.contains(&word_before_replacement.as_str())
        || JOIN_MODIFIERS.contains(&word_before_replacement.as_str())
        || TABLE_INTRODUCERS.contains(&previous.as_str())
        || is_table_list_continuation(tokens, trailing.replacement_start)
    {
        return CursorIntent {
            kind: CursorKind::Table,
            prefix: trailing.prefix,
            replacement_start: trailing.replacement_start,
            qualifier_parts: trailing.qualifier_parts,
            confidence: Confidence::High,
            target_source: None,
        };
    }

    if (previous == "set" || statement_kind == StatementKind::Update)
        && has_word_before(tokens, cursor, "set")
    {
        let idx = sources
            .iter()
            .position(|s| s.is_mutation_target)
            .or(if sources.len() == 1 { Some(0) } else { None });
        return CursorIntent {
            kind: CursorKind::UpdateColumn,
            prefix: trailing.prefix,
            replacement_start: trailing.replacement_start,
            qualifier_parts: trailing.qualifier_parts,
            confidence: if idx.is_some() {
                Confidence::Medium
            } else {
                Confidence::Low
            },
            target_source: idx,
        };
    }

    if matches!(
        word_before_replacement.as_str(),
        "where" | "on" | "and" | "or" | "having" | "by" | "select"
    ) && !sources.is_empty()
        && !has_word_after(tokens, cursor, "from")
    {
        return CursorIntent {
            kind: CursorKind::Column,
            prefix: trailing.prefix,
            replacement_start: trailing.replacement_start,
            qualifier_parts: trailing.qualifier_parts,
            confidence: Confidence::Medium,
            target_source,
        };
    }

    CursorIntent {
        kind: CursorKind::Keyword,
        prefix: trailing.prefix,
        replacement_start: trailing.replacement_start,
        qualifier_parts: trailing.qualifier_parts,
        confidence: Confidence::Low,
        target_source: None,
    }
}

fn trailing_identifier(tokens: &[SemanticToken], cursor: usize) -> TrailingIdentifier {
    let before: Vec<&SemanticToken> = tokens
        .iter()
        .filter(|t| {
            t.span.start < cursor && !matches!(t.kind, TokenKind::Comment | TokenKind::String)
        })
        .collect();
    let Some(last) = before.last().copied() else {
        return TrailingIdentifier {
            prefix: String::new(),
            replacement_start: cursor,
            qualifier_parts: Vec::new(),
            ends_with_dot: false,
        };
    };

    if last.span.end < cursor && last.text != "." {
        return TrailingIdentifier {
            prefix: String::new(),
            replacement_start: cursor,
            qualifier_parts: Vec::new(),
            ends_with_dot: false,
        };
    }

    let mut index = before.len().saturating_sub(1);
    let ends_with_dot = last.text == ".";
    let mut prefix = String::new();
    let mut replacement_start = cursor;

    if is_identifier(last) && cursor <= last.span.end {
        prefix = input_prefix(last, cursor);
        replacement_start = last.span.start;
        if index > 0 && before[index - 1].text == "." {
            index -= 1;
        } else {
            return TrailingIdentifier {
                prefix,
                replacement_start,
                qualifier_parts: Vec::new(),
                ends_with_dot: false,
            };
        }
    } else if ends_with_dot {
        index -= 1;
    }

    let mut qualifier_parts = Vec::new();
    while index < before.len() {
        let token = before[index];
        if !is_identifier(token) {
            break;
        }
        qualifier_parts.insert(0, ident_name(token));
        if index == 0 || before[index - 1].text != "." {
            break;
        }
        index -= 2;
    }

    TrailingIdentifier {
        prefix,
        replacement_start,
        qualifier_parts,
        ends_with_dot,
    }
}

fn input_prefix(token: &SemanticToken, cursor: usize) -> String {
    let byte_len = cursor.saturating_sub(token.span.start);
    let slice = super::ident::byte_prefix(&token.text, byte_len);
    if token.kind == TokenKind::QuotedIdentifier {
        super::ident::unquote_ident(slice)
    } else {
        slice.to_string()
    }
}

fn previous_word(tokens: &[SemanticToken], cursor: usize) -> String {
    word_before(tokens, cursor)
}

fn word_before(tokens: &[SemanticToken], position: usize) -> String {
    tokens
        .iter()
        .rfind(|t| t.span.end <= position && is_identifier(t))
        .map(|t| {
            if t.kind == TokenKind::QuotedIdentifier {
                ident_name(t)
            } else {
                t.normalized.clone()
            }
        })
        .unwrap_or_default()
}

fn has_word_before(tokens: &[SemanticToken], cursor: usize, word: &str) -> bool {
    tokens
        .iter()
        .any(|t| t.span.end <= cursor && t.kind == TokenKind::Word && t.normalized == word)
}

fn has_word_after(tokens: &[SemanticToken], cursor: usize, word: &str) -> bool {
    tokens
        .iter()
        .any(|t| t.span.start >= cursor && t.kind == TokenKind::Word && t.normalized == word)
}

fn is_table_list_continuation(tokens: &[SemanticToken], position: usize) -> bool {
    let before: Vec<&SemanticToken> = tokens
        .iter()
        .filter(|t| t.span.end <= position && t.kind != TokenKind::Comment)
        .collect();
    let Some(comma) = before.last() else {
        return false;
    };
    if comma.text != "," {
        return false;
    }
    let depth = comma.depth;
    for token in before.iter().rev().skip(1) {
        if token.depth != depth || token.kind != TokenKind::Word {
            continue;
        }
        if TABLE_INTRODUCERS.contains(&token.normalized.as_str()) {
            return true;
        }
        if CLAUSE_WORDS.contains(&token.normalized.as_str()) {
            return false;
        }
    }
    false
}

fn source_for_qualifier(sources: &[RowSource], qualifier_parts: &[String]) -> Option<usize> {
    let qualifier = qualifier_parts.last()?;
    sources.iter().position(|s| {
        s.alias
            .as_ref()
            .is_some_and(|a| a.eq_ignore_ascii_case(qualifier))
            || s.name.eq_ignore_ascii_case(qualifier)
    })
}

fn is_row_source_word(sources: &[RowSource], word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    sources.iter().any(|s| {
        s.name.eq_ignore_ascii_case(word)
            || s.alias
                .as_ref()
                .is_some_and(|a| a.eq_ignore_ascii_case(word))
    })
}

fn is_clause_after_row_source(
    tokens: &[SemanticToken],
    cursor: usize,
    sources: &[RowSource],
) -> bool {
    let before: Vec<&SemanticToken> = tokens
        .iter()
        .filter(|t| {
            t.span.end <= cursor && !matches!(t.kind, TokenKind::Comment | TokenKind::String)
        })
        .collect();
    let Some(last) = before.last().copied() else {
        return false;
    };
    if is_identifier(last) && cursor <= last.span.end {
        return false;
    }
    if last.text == "," {
        return false;
    }
    if !is_identifier(last) || !is_row_source_word(sources, &ident_name(last)) {
        return false;
    }
    for token in tokens {
        if token.span.start >= last.span.end && token.span.end <= cursor && token.text == "," {
            return false;
        }
    }
    last.span.end < cursor
}

fn is_identifier(token: &SemanticToken) -> bool {
    matches!(token.kind, TokenKind::Word | TokenKind::QuotedIdentifier)
}

fn ident_name(token: &SemanticToken) -> String {
    if token.kind == TokenKind::QuotedIdentifier {
        super::ident::unquote_ident(&token.text)
    } else {
        token.text.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_table_after_from() {
        let sql = "SELECT * FROM ord";
        let model = build_semantic_model(sql, sql.len());
        assert!(matches!(model.cursor_intent.kind, CursorKind::Table));
        assert_eq!(model.cursor_intent.confidence, Confidence::High);
    }

    #[test]
    fn quoted_chinese_table_row_sources_parsed() {
        let sql = "SELECT * FROM \"测试表\" w";
        let model = build_semantic_model(sql, sql.len());
        assert_eq!(model.row_sources.len(), 1);
        assert_eq!(model.row_sources[0].name, "测试表");
        assert!(matches!(model.cursor_intent.kind, CursorKind::Keyword));
    }

    #[test]
    fn chinese_table_row_sources_parsed() {
        let sql = "SELECT * FROM 测试表 w";
        let model = build_semantic_model(sql, sql.len());
        assert_eq!(model.row_sources.len(), 1);
        assert_eq!(model.row_sources[0].name, "测试表");
        assert!(matches!(model.cursor_intent.kind, CursorKind::Keyword));
    }

    #[test]
    fn read_qualified_name_after_from() {
        let tokens = super::tokens::tokenize_sql("SELECT * FROM users");
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[3].text, "users");
        assert!(is_identifier(&tokens[3]));
        assert!(read_qualified_name(&tokens, 3).is_some());
    }

    #[test]
    fn parse_row_sources_finds_users_after_from() {
        let sql = "SELECT * FROM users";
        let tokens = super::tokens::tokenize_sql(sql);
        let from_idx = tokens
            .iter()
            .position(|t| t.normalized == "from")
            .expect("from");
        let parsed = parse_table_at(&tokens, from_idx + 1, "from", StatementKind::Select);
        assert!(parsed.is_some(), "parse_table_at failed at {from_idx}");
        let sources = parse_row_sources(&tokens, StatementKind::Select);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "users");
    }

    #[test]
    fn semantic_keyword_after_from_table() {
        let sql = "SELECT * FROM users w";
        let model = build_semantic_model(sql, sql.len());
        assert!(matches!(model.cursor_intent.kind, CursorKind::Keyword));
        assert_eq!(model.cursor_intent.confidence, Confidence::High);
        assert_eq!(model.cursor_intent.prefix, "w");
    }

    #[test]
    fn semantic_maps_to_completion_context() {
        let sql = "INSERT INTO users (id, na";
        let cursor = Cursor::new(0, sql.chars().count());
        let legacy = super::super::context::get_completion_context_heuristic(sql, cursor);
        let model = build_semantic_model(sql, super::super::context::cursor_offset(sql, cursor));
        let ctx = completion_context_from_semantic(&model, sql, cursor, legacy);
        assert!(matches!(ctx.intent, CompletionIntent::InsertColumn { .. }));
    }

    #[test]
    fn semantic_input_prefix_unicode() {
        let tokens = tokens::tokenize_sql("SELECT 测试");
        let word = tokens
            .iter()
            .find(|t| t.text == "测试")
            .expect("unicode word token");
        let mid = word.span.start + "测".len();
        let prefix = input_prefix(word, mid);
        assert_eq!(prefix, "测");
    }
}
