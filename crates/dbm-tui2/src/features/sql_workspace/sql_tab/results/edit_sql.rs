//! Build Commit DML for Results row editing (optimistic full-column WHERE).
//! Pure SQL generation, unit-tested without a DB.

use super::edit::ResultsEditState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditTarget {
    pub schema: String,
    pub table: String,
    pub primary_keys: Vec<String>,
    pub columns: Vec<String>,
}

pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn quote_literal(value: &str) -> String {
    if value.eq_ignore_ascii_case("NULL") {
        return "NULL".into();
    }
    format!("'{}'", value.replace('\'', "''"))
}

fn qualified_table(target: &EditTarget) -> String {
    format!(
        "{}.{}",
        quote_ident(&target.schema),
        quote_ident(&target.table)
    )
}

fn column_index(columns: &[String], name: &str) -> Option<usize> {
    columns.iter().position(|c| c.eq_ignore_ascii_case(name))
}

fn old_value_predicate(column: &str, value: &str) -> String {
    let ident = quote_ident(column);
    if value.eq_ignore_ascii_case("NULL") {
        format!("{ident} IS NULL")
    } else {
        format!("{ident} IS NOT DISTINCT FROM {}", quote_literal(value))
    }
}

fn full_row_where(target: &EditTarget, snapshot: &[String]) -> Result<String, String> {
    if target.columns.len() != snapshot.len() {
        return Err("snapshot column count mismatch".into());
    }
    for pk in &target.primary_keys {
        let idx = column_index(&target.columns, pk)
            .ok_or_else(|| format!("primary key `{pk}` missing from result columns"))?;
        if snapshot
            .get(idx)
            .is_some_and(|v| v.eq_ignore_ascii_case("NULL"))
        {
            return Err(format!("primary key `{pk}` is NULL"));
        }
    }
    let parts: Vec<String> = target
        .columns
        .iter()
        .enumerate()
        .map(|(i, col)| old_value_predicate(col, &snapshot[i]))
        .collect();
    Ok(parts.join(" AND "))
}

/// Build ordered statements: UPDATEs, DELETEs, then INSERTs.
pub fn build_commit_statements(
    target: &EditTarget,
    state: &ResultsEditState,
) -> Result<Vec<String>, String> {
    if target.primary_keys.is_empty() {
        return Err("no primary key".into());
    }
    let table = qualified_table(target);
    let mut statements = Vec::new();

    // UPDATEs for dirty non-deleted rows.
    let mut dirty_rows: Vec<usize> = state
        .dirty_cells
        .keys()
        .map(|(r, _)| *r)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    dirty_rows.sort_unstable();
    for row_idx in dirty_rows {
        if state.deleted.contains(&row_idx) {
            continue;
        }
        let snapshot = state
            .snapshots
            .get(row_idx)
            .ok_or_else(|| format!("missing snapshot for row {row_idx}"))?;
        let sets: Vec<String> = state
            .dirty_cells
            .iter()
            .filter(|((r, _), _)| *r == row_idx)
            .filter_map(|((_, c), v)| {
                let col = target.columns.get(*c)?;
                if target
                    .primary_keys
                    .iter()
                    .any(|pk| pk.eq_ignore_ascii_case(col))
                {
                    return None; // do not UPDATE PK columns
                }
                Some(format!("{} = {}", quote_ident(col), quote_literal(v)))
            })
            .collect();
        if sets.is_empty() {
            continue;
        }
        let where_clause = full_row_where(target, snapshot)?;
        statements.push(format!(
            "UPDATE {table} SET {} WHERE {where_clause}",
            sets.join(", ")
        ));
    }

    let mut deleted: Vec<usize> = state.deleted.iter().copied().collect();
    deleted.sort_unstable();
    for row_idx in deleted {
        let snapshot = state
            .snapshots
            .get(row_idx)
            .ok_or_else(|| format!("missing snapshot for row {row_idx}"))?;
        let where_clause = full_row_where(target, snapshot)?;
        statements.push(format!("DELETE FROM {table} WHERE {where_clause}"));
    }

    for new_row in &state.new_rows {
        let pairs: Vec<(&str, &str)> = target
            .columns
            .iter()
            .zip(new_row.iter())
            .filter(|(_, v)| !v.is_empty() && !v.eq_ignore_ascii_case("NULL"))
            .map(|(c, v)| (c.as_str(), v.as_str()))
            .collect();
        if pairs.is_empty() {
            continue;
        }
        let cols = pairs
            .iter()
            .map(|(c, _)| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        let vals = pairs
            .iter()
            .map(|(_, v)| quote_literal(v))
            .collect::<Vec<_>>()
            .join(", ");
        statements.push(format!("INSERT INTO {table} ({cols}) VALUES ({vals})"));
    }

    if statements.is_empty() {
        return Err("nothing to commit".into());
    }
    Ok(statements)
}

pub fn join_batch_sql(statements: &[String]) -> String {
    statements.join(";\n")
}

/// Re-exported from `common::utils::sql_editability`: `UPDATE`/`DELETE` must
/// affect exactly 1 row during commit conflict checking.
pub use crate::common::utils::sql_editability::statement_requires_one_row;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_where_includes_all_column_old_values() {
        let target = EditTarget {
            schema: "public".into(),
            table: "users".into(),
            primary_keys: vec!["id".into()],
            columns: vec!["id".into(), "name".into()],
        };
        let mut s = ResultsEditState::default();
        s.enter_edit(&[vec!["1".into(), "Ada".into()]]);
        s.apply_cell(0, 1, "Bob".into());
        let stmts = build_commit_statements(&target, &s).unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].contains("UPDATE"));
        assert!(stmts[0].contains("IS NOT DISTINCT FROM"));
        assert!(stmts[0].contains("'Ada'"));
        assert!(stmts[0].contains("'Bob'"));
    }

    #[test]
    fn join_batch_sql_joins_with_semicolons() {
        assert_eq!(join_batch_sql(&["A".into(), "B".into()]), "A;\nB");
    }

    #[test]
    fn statement_requires_one_row_classifies_update_delete() {
        assert!(statement_requires_one_row("UPDATE users SET x = 1"));
        assert!(statement_requires_one_row("DELETE FROM users"));
        assert!(!statement_requires_one_row(
            "INSERT INTO users (a) VALUES (1)"
        ));
    }
}
