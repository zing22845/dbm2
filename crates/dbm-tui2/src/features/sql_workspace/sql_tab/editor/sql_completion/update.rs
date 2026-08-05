//! SQL completion sub-module update.
//!
//! Pure by-value transition. `Refresh` recomputes the completion items from the
//! SQL buffer, cursor and available metadata; the resolved items drive whether
//! the popup opens or stays closed.

use crate::common::utils::cursor::Cursor;

use super::provider::ColumnInfo;

use super::msg::SqlCompletionMessage;
use super::state::SqlCompletionState;
use super::intent::SqlCompletionIntent;
use super::effect::SqlCompletionEffect;
use super::context::{CompletionIntent, get_completion_context, should_offer_completion_explicit};
use super::engine::SqlEngine;
use super::provider::{CompletionInput, build_completion_items};

/// Update the SQL completion state. Pure by-value transition.
pub fn update(
    msg: SqlCompletionMessage,
    mut state: SqlCompletionState,
) -> (SqlCompletionState, Vec<SqlCompletionIntent>, Vec<SqlCompletionEffect>) {
    let mut intents = Vec::new();
    let effects = Vec::new();

    match msg {
        SqlCompletionMessage::Refresh {
            sql,
            cursor,
            tables,
            columns,
        } => {
            refresh(&mut state, &sql, cursor, &tables, &columns);
        }
        SqlCompletionMessage::Close => {
            state.close();
        }
        SqlCompletionMessage::MoveSelection { delta } => {
            if state.is_open() {
                state.move_selection(delta);
            }
        }
        SqlCompletionMessage::Apply => {
            if let Some(item) = state.selected_item().cloned() {
                let replace_start = state.replace_start;
                let replace_end = state.replace_end;
                intents.push(SqlCompletionIntent::Apply {
                    item,
                    replace_start,
                    replace_end,
                });
            }
            state.close();
        }
    }

    (state, intents, effects)
}

/// Recompute completion items and update the popup open/closed state.
fn refresh(
    state: &mut SqlCompletionState,
    sql: &str,
    cursor: Cursor,
    tables: &[String],
    columns: &[ColumnInfo],
) {
    let context = get_completion_context(sql, cursor);

    // Suppressed contexts never show a popup.
    if matches!(context.intent, CompletionIntent::Suppressed) {
        state.close();
        return;
    }

    // Only offer keyword completion with a prefix (or when explicit); the
    // editor triggers an explicit refresh with Shift+Tab. We conservatively
    // require a prefix for keyword intent to avoid an empty popup.
    if matches!(context.intent, CompletionIntent::Keyword)
        && context.prefix.is_empty()
        && !should_offer_completion_explicit(&context, sql)
    {
        state.close();
        return;
    }

    let referenced = extract_referenced_before(sql, cursor);
    let items = build_completion_items(
        &context,
        &CompletionInput {
            engine: SqlEngine::Postgres,
            tables,
            columns,
            referenced_tables: &referenced,
        },
        false,
        sql,
        cursor,
    );

    if items.is_empty() {
        state.close();
        return;
    }

    let replace_start = context.replace_start;
    *state = SqlCompletionState::open_with(items, replace_start, cursor);
}

/// Tables referenced before the cursor, used for alias snippets and qualifier
/// resolution.
fn extract_referenced_before(
    sql: &str,
    cursor: Cursor,
) -> Vec<super::context::TableRef> {
    let offset = super::context::cursor_offset(sql, cursor);
    super::context::extract_referenced_tables(&sql[..offset])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor_at(sql: &str) -> Cursor {
        Cursor::new(0, sql.chars().count())
    }

    #[test]
    fn refresh_opens_keyword_popup_with_prefix() {
        let (s, _i, _e) = update(
            SqlCompletionMessage::Refresh {
                sql: "select * from users wh".into(),
                cursor: cursor_at("select * from users wh"),
                tables: vec![],
                columns: vec![],
            },
            SqlCompletionState::default(),
        );
        assert!(s.is_open());
        assert!(s.items.iter().any(|i| i.label == "WHERE"));
    }

    #[test]
    fn refresh_closes_when_suppressed() {
        let (s, _i, _e) = update(
            SqlCompletionMessage::Refresh {
                sql: "SELECT 'foo".into(),
                cursor: cursor_at("SELECT 'foo"),
                tables: vec![],
                columns: vec![],
            },
            SqlCompletionState::default(),
        );
        assert!(!s.is_open());
    }

    #[test]
    fn move_selection_wraps() {
        let (mut s, _i, _e) = update(
            SqlCompletionMessage::Refresh {
                sql: "sel".into(),
                cursor: cursor_at("sel"),
                tables: vec![],
                columns: vec![],
            },
            SqlCompletionState::default(),
        );
        assert!(s.is_open());
        let len = s.items.len();
        s.move_selection(1);
        assert_eq!(s.selected, 1);
        s.move_selection(-2);
        assert_eq!(s.selected, (len + 1 - 2) % len);
    }

    #[test]
    fn close_clears_items() {
        let (mut s, _i, _e) = update(
            SqlCompletionMessage::Refresh {
                sql: "sel".into(),
                cursor: cursor_at("sel"),
                tables: vec![],
                columns: vec![],
            },
            SqlCompletionState::default(),
        );
        assert!(s.is_open());
        let (s2, _i, _e) = update(SqlCompletionMessage::Close, s);
        s = s2;
        assert!(!s.is_open());
        assert!(s.items.is_empty());
    }
}
