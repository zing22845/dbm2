//! Completion item providers: build the ranked list of items for a context.

use std::collections::HashSet;

use crate::common::utils::cursor::Cursor;

use super::alias;
use super::context::{
    CompletionContext, CompletionIntent, TableRef, is_select_list_column_context,
};
use super::engine::SqlEngine;
use super::keywords::{CLAUSE_KEYWORDS, keyword_rank, keywords_for_engine};
use super::match_score::{match_score, matches_prefix};
use super::preferred::preferred_keywords_for_completion;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionKind {
    Keyword,
    Table,
    Column,
    Alias,
    #[allow(dead_code)]
    Schema,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    pub insert_text: String,
}

/// The subset of column metadata the completion engine needs, as an `Eq` value
/// so it can travel through the (Eq-based) message router. Derived from
/// `dbm_core::ColumnMeta` by the editor when preparing a refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    pub name: String,
    pub type_name: String,
    pub type_display: String,
    pub comment: Option<String>,
}

impl From<dbm_core::ColumnMeta> for ColumnInfo {
    fn from(m: dbm_core::ColumnMeta) -> Self {
        ColumnInfo {
            name: m.name,
            type_name: m.type_name,
            type_display: m.type_display,
            comment: m.comment,
        }
    }
}

pub struct CompletionInput<'a> {
    pub engine: SqlEngine,
    pub tables: &'a [String],
    pub columns: &'a [ColumnInfo],
    pub referenced_tables: &'a [TableRef],
}

pub fn build_keyword_items(
    prefix: &str,
    sql: &str,
    cursor: Cursor,
    context: &CompletionContext,
    clause_only: bool,
    referenced_tables: &[TableRef],
    engine: SqlEngine,
) -> Vec<CompletionItem> {
    let exclusive_table = matches!(context.intent, CompletionIntent::Table { .. });
    build_keyword_completion_items(
        prefix,
        sql,
        cursor,
        context,
        clause_only,
        exclusive_table,
        referenced_tables,
        engine,
    )
}

pub fn build_completion_items(
    context: &CompletionContext,
    input: &CompletionInput<'_>,
    keyword_clause_only: bool,
    sql: &str,
    cursor: Cursor,
) -> Vec<CompletionItem> {
    match &context.intent {
        CompletionIntent::Suppressed => Vec::new(),
        CompletionIntent::Keyword => {
            let clause_only = keyword_clause_only && context.prefix.is_empty();
            let exclusive_table = false;
            build_keyword_completion_items(
                &context.prefix,
                sql,
                cursor,
                context,
                clause_only,
                exclusive_table,
                input.referenced_tables,
                input.engine,
            )
        }
        CompletionIntent::Table { schema } => {
            let _ = schema;
            build_tables(&context.prefix, input.tables)
        }
        CompletionIntent::Column { tables } => {
            let table_ctx = tables.first();
            let mut items = if input.columns.is_empty() {
                Vec::new()
            } else {
                build_columns(&context.prefix, input.columns, table_ctx)
            };
            if is_select_list_column_context(sql, cursor)
                && !tables.is_empty()
                && !input.columns.is_empty()
            {
                items.extend(build_table_star_items(
                    &context.prefix,
                    tables,
                    input.columns,
                ));
            }
            items
        }
        CompletionIntent::InsertColumn { .. } | CompletionIntent::UpdateColumn { .. } => {
            let table_ctx = match &context.intent {
                CompletionIntent::InsertColumn {
                    table,
                    schema,
                    alias,
                } => Some(TableRef {
                    name: table.clone(),
                    schema: schema.clone(),
                    alias: alias.clone(),
                }),
                CompletionIntent::UpdateColumn { table, schema } => Some(TableRef {
                    name: table.clone(),
                    schema: schema.clone(),
                    alias: None,
                }),
                _ => None,
            };
            let mut items = build_columns(&context.prefix, input.columns, table_ctx.as_ref());
            if let Some(table_ref) = table_ctx
                && matches!(context.intent, CompletionIntent::InsertColumn { .. })
                    && !input.columns.is_empty()
            {
                items.extend(build_table_star_items(
                    &context.prefix,
                    &[table_ref],
                    input.columns,
                ));
            }
            items
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_keyword_completion_items(
    prefix: &str,
    sql: &str,
    cursor: Cursor,
    _context: &CompletionContext,
    clause_only: bool,
    exclusive_table: bool,
    referenced_tables: &[TableRef],
    engine: SqlEngine,
) -> Vec<CompletionItem> {
    let preferred = preferred_keywords_for_completion(sql, cursor, exclusive_table);
    let mut scored = Vec::new();
    let mut seen = HashSet::new();

    for (index, keyword) in preferred.iter().enumerate() {
        if !matches_prefix(keyword, prefix) {
            continue;
        }
        let upper = keyword.to_ascii_uppercase();
        if !seen.insert(upper.clone()) {
            continue;
        }
        scored.push(ScoredItem {
            sort_key: 10_000 - index as i32,
            item: CompletionItem {
                label: upper,
                insert_text: keyword.to_string(),
                kind: CompletionKind::Keyword,
                detail: Some("preferred".into()),
            },
        });
    }

    if should_suggest_alias_snippets(referenced_tables) {
        for alias in alias::build_alias_items(prefix, referenced_tables) {
            let key = alias.label.to_ascii_lowercase();
            if !seen.insert(key) {
                continue;
            }
            scored.push(ScoredItem {
                sort_key: 5_000,
                item: CompletionItem {
                    label: alias.label,
                    insert_text: alias.insert_text,
                    kind: CompletionKind::Alias,
                    detail: Some(alias.detail),
                },
            });
        }
    }

    let source: Vec<&str> = if clause_only {
        CLAUSE_KEYWORDS.to_vec()
    } else {
        keywords_for_engine(engine)
    };
    for keyword in source {
        let upper = keyword.to_ascii_uppercase();
        if !seen.insert(upper.clone()) {
            continue;
        }
        if !matches_prefix(keyword, prefix) {
            continue;
        }
        scored.push(ScoredItem {
            sort_key: match_score(keyword, prefix)
                + if keyword_rank(keyword) == 0 { 200 } else { 0 },
            item: CompletionItem {
                label: upper,
                insert_text: keyword.to_string(),
                kind: CompletionKind::Keyword,
                detail: None,
            },
        });
    }

    scored.sort_by(|a, b| {
        b.sort_key
            .cmp(&a.sort_key)
            .then_with(|| a.item.label.len().cmp(&b.item.label.len()))
            .then_with(|| a.item.label.cmp(&b.item.label))
    });
    scored.truncate(50);
    scored.into_iter().map(|s| s.item).collect()
}

struct ScoredItem {
    sort_key: i32,
    item: CompletionItem,
}

fn should_suggest_alias_snippets(referenced_tables: &[TableRef]) -> bool {
    !referenced_tables.is_empty()
}

pub fn tables_needed(context: &CompletionContext, default_schema: &str) -> Vec<(String, String)> {
    match &context.intent {
        CompletionIntent::Table { schema } => {
            vec![(
                schema.clone().unwrap_or_else(|| default_schema.to_string()),
                String::new(),
            )]
        }
        CompletionIntent::InsertColumn { table, schema, .. } => {
            vec![(
                schema.clone().unwrap_or_else(|| default_schema.to_string()),
                table.clone(),
            )]
        }
        CompletionIntent::UpdateColumn { table, schema } => {
            vec![(
                schema.clone().unwrap_or_else(|| default_schema.to_string()),
                table.clone(),
            )]
        }
        CompletionIntent::Column { tables } => tables
            .iter()
            .map(|t| {
                (
                    t.schema
                        .clone()
                        .unwrap_or_else(|| default_schema.to_string()),
                    t.name.clone(),
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub fn needs_table_metadata(context: &CompletionContext) -> bool {
    matches!(
        context.intent,
        CompletionIntent::Table { .. }
            | CompletionIntent::Column { .. }
            | CompletionIntent::InsertColumn { .. }
            | CompletionIntent::UpdateColumn { .. }
    )
}

fn build_tables(prefix: &str, tables: &[String]) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = tables
        .iter()
        .filter(|name| matches_prefix(name, prefix))
        .map(|name| CompletionItem {
            label: name.clone(),
            insert_text: quote_if_needed(name),
            kind: CompletionKind::Table,
            detail: Some("table".into()),
        })
        .collect();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items.truncate(50);
    items
}

fn build_table_star_items(
    prefix: &str,
    tables: &[TableRef],
    columns: &[ColumnInfo],
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let mut emitted = HashSet::new();

    for table in tables {
        let display = table.alias.as_deref().unwrap_or(&table.name);
        let label = format!("{display}.*");
        let key = label.to_ascii_lowercase();
        if emitted.contains(&key) {
            continue;
        }
        if !select_all_matches_prefix(&label, display, columns, prefix) {
            continue;
        }
        emitted.insert(key);

        let expansion: Vec<String> = columns
            .iter()
            .map(|col| quote_if_needed(&col.name))
            .collect();
        let expansion_text = expansion.join(", ");
        let detail = if expansion_text.len() > 60 {
            format!("columns: {}...", &expansion_text[..57])
        } else {
            format!("columns: {expansion_text}")
        };
        items.push(CompletionItem {
            label,
            insert_text: expansion_text,
            kind: CompletionKind::Column,
            detail: Some(detail),
        });
    }

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

fn select_all_matches_prefix(
    label: &str,
    table_name: &str,
    columns: &[ColumnInfo],
    prefix: &str,
) -> bool {
    if prefix.is_empty() {
        return true;
    }
    if matches_prefix(label, prefix) || matches_prefix(table_name, prefix) {
        return true;
    }
    columns.iter().any(|col| matches_prefix(&col.name, prefix))
}

fn build_columns(
    prefix: &str,
    columns: &[ColumnInfo],
    table: Option<&TableRef>,
) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = columns
        .iter()
        .filter(|col| matches_prefix(&col.name, prefix))
        .map(|col| CompletionItem {
            label: col.name.clone(),
            insert_text: quote_if_needed(&col.name),
            kind: CompletionKind::Column,
            detail: Some(column_detail(col, table)),
        })
        .collect();
    items.sort_by(|a, b| {
        match_score(&b.label, prefix)
            .cmp(&match_score(&a.label, prefix))
            .then_with(|| a.label.cmp(&b.label))
    });
    items.truncate(50);
    items
}

fn column_detail(col: &ColumnInfo, table: Option<&TableRef>) -> String {
    let type_part = if col.type_display.is_empty() {
        col.type_name.as_str()
    } else {
        col.type_display.as_str()
    };
    let head = match table {
        Some(t) => {
            let schema = t.schema.as_deref().unwrap_or("public");
            format!("{schema}.{} [{type_part}]", t.name)
        }
        None => type_part.to_string(),
    };
    match col.comment.as_deref() {
        Some(comment) if !comment.is_empty() => format!("{head} -- {comment}"),
        _ => head,
    }
}

fn quote_if_needed(name: &str) -> String {
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

#[cfg(test)]
mod tests {
    use super::super::context::extract_referenced_tables;
    use super::*;

    fn cursor_at(sql: &str) -> Cursor {
        Cursor::new(0, sql.chars().count())
    }

    #[test]
    fn column_detail_includes_comment_and_table_context() {
        let col = ColumnInfo {
            name: "id".into(),
            type_name: "integer".into(),
            type_display: "integer".into(),
            comment: Some("主键ID".into()),
        };
        let table = TableRef {
            name: "测试表".into(),
            schema: Some("public".into()),
            alias: None,
        };
        let detail = column_detail(&col, Some(&table));
        assert!(detail.contains("public.测试表"));
        assert!(detail.contains("integer"));
        assert!(detail.contains("主键ID"));
    }

    #[test]
    fn fuzzy_keyword_typo_still_matches_select() {
        let ctx = CompletionContext {
            prefix: "selct".into(),
            replace_start: Cursor::new(0, 0),
            qualifier_parts: Vec::new(),
            intent: CompletionIntent::Keyword,
        };
        let items = build_completion_items(
            &ctx,
            &CompletionInput {
                engine: SqlEngine::Postgres,
                tables: &[],
                columns: &[],
                referenced_tables: &[],
            },
            false,
            "selct",
            cursor_at("selct"),
        );
        assert!(items.iter().any(|i| i.label == "SELECT"));
    }

    #[test]
    fn filters_keywords_by_prefix() {
        let ctx = CompletionContext {
            prefix: "sel".into(),
            replace_start: Cursor::new(0, 0),
            qualifier_parts: Vec::new(),
            intent: CompletionIntent::Keyword,
        };
        let items = build_completion_items(
            &ctx,
            &CompletionInput {
                engine: SqlEngine::Postgres,
                tables: &[],
                columns: &[],
                referenced_tables: &[],
            },
            false,
            "select sel",
            cursor_at("select sel"),
        );
        assert!(items.iter().any(|i| i.label == "SELECT"));
    }

    #[test]
    fn substring_matches_where_for_wh_prefix() {
        let ctx = CompletionContext {
            prefix: "wh".into(),
            replace_start: Cursor::new(0, 0),
            qualifier_parts: Vec::new(),
            intent: CompletionIntent::Keyword,
        };
        let sql = "delete from users wh";
        let items = build_completion_items(
            &ctx,
            &CompletionInput {
                engine: SqlEngine::Postgres,
                tables: &[],
                columns: &[],
                referenced_tables: &extract_referenced_tables(sql),
            },
            false,
            sql,
            cursor_at(sql),
        );
        assert!(items.iter().any(|i| i.label == "WHERE"));
    }

    #[test]
    fn explicit_after_from_users_includes_alias_snippet() {
        let sql = "select * from users ";
        let cursor = cursor_at(sql);
        let ctx = CompletionContext {
            prefix: String::new(),
            replace_start: cursor,
            qualifier_parts: Vec::new(),
            intent: CompletionIntent::Keyword,
        };
        let refs = extract_referenced_tables(sql);
        let items = build_keyword_completion_items(
            "",
            sql,
            cursor,
            &ctx,
            true,
            false,
            &refs,
            SqlEngine::Postgres,
        );
        assert!(items.iter().any(|i| i.kind == CompletionKind::Alias));
        assert!(items.iter().any(|i| i.label == "WHERE"));
    }

    #[test]
    fn after_insert_column_list_suggests_values_not_columns() {
        let sql = "INSERT INTO users (id, c1) v";
        let cursor = cursor_at(sql);
        let ctx = CompletionContext {
            prefix: "v".into(),
            replace_start: Cursor::new(0, sql[..sql.len() - 1].chars().count()),
            qualifier_parts: Vec::new(),
            intent: CompletionIntent::Keyword,
        };
        let items = build_completion_items(
            &ctx,
            &CompletionInput {
                engine: SqlEngine::Postgres,
                tables: &[],
                columns: &[],
                referenced_tables: &[],
            },
            false,
            sql,
            cursor,
        );
        assert!(items.iter().any(|i| i.label == "VALUES"));
        assert!(!items.iter().any(|i| i.kind == CompletionKind::Column));
    }

    #[test]
    fn insert_column_list_includes_alias_star_expansion() {
        let sql = "insert into 测试表 as tb (";
        let cursor_idx = cursor_at(sql);
        let ctx = CompletionContext {
            prefix: String::new(),
            replace_start: cursor_idx,
            qualifier_parts: Vec::new(),
            intent: CompletionIntent::InsertColumn {
                table: "测试表".into(),
                schema: Some("public".into()),
                alias: Some("tb".into()),
            },
        };
        let columns = vec![
            ColumnInfo {
                name: "id".into(),
                type_name: "integer".into(),
                type_display: "integer".into(),
                comment: None,
            },
            ColumnInfo {
                name: "名称".into(),
                type_name: "text".into(),
                type_display: "text".into(),
                comment: None,
            },
        ];
        let items = build_completion_items(
            &ctx,
            &CompletionInput {
                engine: SqlEngine::Postgres,
                tables: &[],
                columns: &columns,
                referenced_tables: &[],
            },
            false,
            sql,
            cursor_idx,
        );
        let star = items
            .iter()
            .find(|i| i.label == "tb.*")
            .expect("alias.* expansion");
        assert!(star.insert_text.contains("id"));
        assert!(star.insert_text.contains("\"名称\""));
    }

    #[test]
    fn select_list_includes_table_star_expansion() {
        let sql = "select id from \"测试表\"";
        let cursor = "select id".len();
        let cursor_idx = Cursor::new(0, sql[..cursor].chars().count());
        let ctx = CompletionContext {
            prefix: "id".into(),
            replace_start: Cursor::new(0, "select ".chars().count()),
            qualifier_parts: Vec::new(),
            intent: CompletionIntent::Column {
                tables: vec![TableRef {
                    name: "测试表".into(),
                    schema: Some("public".into()),
                    alias: None,
                }],
            },
        };
        let columns = vec![
            ColumnInfo {
                name: "id".into(),
                type_name: "integer".into(),
                type_display: "integer".into(),
                comment: None,
            },
            ColumnInfo {
                name: "名称".into(),
                type_name: "text".into(),
                type_display: "text".into(),
                comment: None,
            },
        ];
        let items = build_completion_items(
            &ctx,
            &CompletionInput {
                engine: SqlEngine::Postgres,
                tables: &[],
                columns: &columns,
                referenced_tables: &[],
            },
            false,
            sql,
            cursor_idx,
        );
        assert!(items.iter().any(|i| i.label == "id"));
        let star = items
            .iter()
            .find(|i| i.label == "测试表.*")
            .expect("table.* expansion");
        assert!(star.insert_text.contains("id"));
        assert!(star.insert_text.contains("\"名称\""));
    }
}
