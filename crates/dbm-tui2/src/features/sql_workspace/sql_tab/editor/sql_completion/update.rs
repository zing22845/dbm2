//! SQL completion sub-module update.
//!
//! Pure by-value transition. `Refresh` recomputes the completion items from the
//! SQL buffer, cursor and available metadata; the resolved items drive whether
//! the popup opens or stays closed.

use crate::common::utils::cursor::Cursor;

use super::provider::ColumnInfo;

use super::context::{
    CompletionIntent, get_completion_context, should_auto_open, should_offer_completion_explicit,
};
use super::effect::SqlCompletionEffect;
use super::engine::SqlEngine;
use super::intent::SqlCompletionIntent;
use super::msg::SqlCompletionMessage;
use super::provider::{CompletionInput, build_completion_items};
use super::state::SqlCompletionState;

/// Update the SQL completion state. Pure by-value transition.
///
/// The returned `bool` is `dirty`: whether the completion popup's rendered
/// state changed. A `MoveSelection`/`Apply` on a closed popup is a no-op.
pub fn update(
    msg: SqlCompletionMessage,
    mut state: SqlCompletionState,
) -> (
    SqlCompletionState,
    Vec<SqlCompletionIntent>,
    Vec<SqlCompletionEffect>,
    bool,
) {
    let mut intents = Vec::new();
    let effects = Vec::new();

    let dirty = match msg {
        SqlCompletionMessage::Refresh {
            sql,
            cursor,
            tables,
            columns,
            explicit,
            complete_table_names,
        } => {
            refresh(
                &mut state,
                &sql,
                cursor,
                &tables,
                &columns,
                explicit,
                complete_table_names,
            );
            true
        }
        SqlCompletionMessage::Close => {
            let changed = state.is_open();
            state.close();
            changed
        }
        SqlCompletionMessage::MoveSelection { delta } => {
            if state.is_open() {
                let before = state.selected;
                state.move_selection(delta);
                before != state.selected
            } else {
                false
            }
        }
        SqlCompletionMessage::Apply => {
            let was_open = state.is_open();
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
            was_open
        }
    };

    (state, intents, effects, dirty)
}

/// Recompute completion items and update the popup open/closed state.
///
/// `explicit` mirrors the original dbm's `completion_trigger_key` (Shift+Tab):
/// an explicit request forces the popup open even where the auto-open gate
/// would keep it closed (e.g. after `,`/`(`), but still never shows a popup in
/// a suppressed context.
fn refresh(
    state: &mut SqlCompletionState,
    sql: &str,
    cursor: Cursor,
    tables: &[String],
    columns: &[ColumnInfo],
    explicit: bool,
    complete_table_names: bool,
) {
    let context = get_completion_context(sql, cursor);

    // Suppressed contexts never show a popup, explicit or not.
    if matches!(context.intent, CompletionIntent::Suppressed) {
        state.close();
        return;
    }

    // Auto-open gating, matching the original dbm: a *closed* popup only opens
    // after an auto-open trigger (after `from `/`on `/clause keyword, an
    // identifier char, a qualifier / trigger char like `.`/`$`/`@`, or inside a
    // select column list). An already-open popup stays open while typing so the
    // user can keep selecting candidates without it flickering closed. An
    // explicit request (Shift+Tab) bypasses this gate.
    if !explicit && !state.is_open() && !should_auto_open(sql, cursor) {
        state.close();
        return;
    }

    // Only offer keyword completion with a prefix — except on an explicit
    // request (Shift+Tab), which forces the keyword list open even with an
    // empty prefix / empty buffer (matching the original dbm's
    // `completion_trigger_key`). We conservatively require a prefix for the
    // automatic keyword popup to avoid an empty one.
    if matches!(context.intent, CompletionIntent::Keyword)
        && context.prefix.is_empty()
        && !explicit
        && !should_offer_completion_explicit(&context, sql)
    {
        state.close();
        return;
    }

    // A table-intent slot only offers table names when TblCmp is ON. With it
    // OFF, a non-explicit refresh closes the popup (no hint at all), matching
    // the original dbm's `table_completion_allowed` → `build_completion_state_inner`.
    // Only an explicit Shift+Tab request forces keyword completion here.
    if matches!(context.intent, CompletionIntent::Table { .. })
        && !complete_table_names
        && !explicit
    {
        state.close();
        return;
    }
    // explicit: fall through to keyword completion (provider handles it).

    let referenced = extract_referenced_before(sql, cursor);
    let items = build_completion_items(
        &context,
        &CompletionInput {
            engine: SqlEngine::Postgres,
            tables,
            columns,
            referenced_tables: &referenced,
            complete_table_names,
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
fn extract_referenced_before(sql: &str, cursor: Cursor) -> Vec<super::context::TableRef> {
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
        let (s, _i, _e, _d) = update(
            SqlCompletionMessage::Refresh {
                sql: "select * from users wh".into(),
                cursor: cursor_at("select * from users wh"),
                tables: vec![],
                columns: vec![],
                explicit: false,
                complete_table_names: false,
            },
            SqlCompletionState::default(),
        );
        assert!(s.is_open());
        assert!(s.items.iter().any(|i| i.label == "WHERE"));
    }

    #[test]
    fn refresh_closes_when_suppressed() {
        let (s, _i, _e, _d) = update(
            SqlCompletionMessage::Refresh {
                sql: "SELECT 'foo".into(),
                cursor: cursor_at("SELECT 'foo"),
                tables: vec![],
                columns: vec![],
                explicit: false,
                complete_table_names: false,
            },
            SqlCompletionState::default(),
        );
        assert!(!s.is_open());
    }

    #[test]
    fn refresh_does_not_auto_open_after_punctuation() {
        // Right after a comma the auto-open gate stays closed (matching the
        // original dbm): a keyword/column popup must not pop up after `,`.
        let (s, _i, _e, _d) = update(
            SqlCompletionMessage::Refresh {
                sql: "select a,".into(),
                cursor: cursor_at("select a,"),
                tables: vec!["users".into()],
                columns: vec![],
                explicit: false,
                complete_table_names: true,
            },
            SqlCompletionState::default(),
        );
        assert!(!s.is_open(), "must not auto-open after a comma");
    }

    #[test]
    fn refresh_auto_opens_after_from_whitespace() {
        // After `from ` the table popup auto-opens when TblCmp is on (original
        // dbm behavior).
        let (s, _i, _e, _d) = update(
            SqlCompletionMessage::Refresh {
                sql: "select * from ".into(),
                cursor: cursor_at("select * from "),
                tables: vec!["users".into(), "orders".into()],
                columns: vec![],
                explicit: false,
                complete_table_names: true,
            },
            SqlCompletionState::default(),
        );
        assert!(s.is_open());
        assert!(s.items.iter().any(|i| i.label == "users"));
    }

    #[test]
    fn refresh_table_intent_closed_when_tblcmp_off() {
        // With TblCmp OFF, a table-intent slot (`from `) must NOT show any
        // popup at all (no table names, no keywords) — matching the original
        // dbm's `table_completion_allowed` → `build_completion_state_inner`.
        let (s, _i, _e, _d) = update(
            SqlCompletionMessage::Refresh {
                sql: "select * from ".into(),
                cursor: cursor_at("select * from "),
                tables: vec!["users".into(), "orders".into()],
                columns: vec![],
                explicit: false,
                complete_table_names: false,
            },
            SqlCompletionState::default(),
        );
        assert!(
            !s.is_open(),
            "table-intent slot with TblCmp off must not open a popup"
        );
        assert!(
            !s.items
                .iter()
                .any(|i| i.label == "users" || i.label == "orders"),
            "table names must not be offered with TblCmp off"
        );
    }

    #[test]
    fn explicit_refresh_forces_open_on_empty_buffer() {
        // Shift+Tab on an empty buffer must still force the keyword popup open
        // (matching the original dbm), even though an empty buffer would not
        // auto-open or pass the keyword-prefix gate.
        let (s, _i, _e, _d) = update(
            SqlCompletionMessage::Refresh {
                sql: String::new(),
                cursor: Cursor::new(0, 0),
                tables: vec![],
                columns: vec![],
                explicit: true,
                complete_table_names: false,
            },
            SqlCompletionState::default(),
        );
        assert!(
            s.is_open(),
            "explicit refresh on empty buffer must open the popup"
        );
    }

    #[test]
    fn explicit_refresh_forces_open_after_punctuation() {
        // Shift+Tab (explicit) forces the popup open even right after a comma,
        // bypassing the auto-open gate (matching the original dbm).
        let (s, _i, _e, _d) = update(
            SqlCompletionMessage::Refresh {
                sql: "select a,".into(),
                cursor: cursor_at("select a,"),
                tables: vec!["users".into()],
                columns: vec![],
                explicit: true,
                complete_table_names: true,
            },
            SqlCompletionState::default(),
        );
        assert!(s.is_open(), "explicit refresh must force the popup open");
    }

    #[test]
    fn move_selection_wraps() {
        let (mut s, _i, _e, _d) = update(
            SqlCompletionMessage::Refresh {
                sql: "sel".into(),
                cursor: cursor_at("sel"),
                tables: vec![],
                columns: vec![],
                explicit: false,
                complete_table_names: false,
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
    fn refresh_offers_columns_after_select_space_with_from() {
        // `SELECT <cursor> FROM t`: moving the cursor behind `SELECT ` must pop
        // the column list (the original dbm's should_offer_completion treats a
        // Column intent as always offering).
        let (s, _i, _e, _d) = update(
            SqlCompletionMessage::Refresh {
                sql: "SELECT  FROM tb1".into(),
                cursor: cursor_at("SELECT "),
                tables: vec!["tb1".into()],
                columns: vec![ColumnInfo {
                    name: "status".into(),
                    type_name: "text".into(),
                    type_display: "text".into(),
                    comment: None,
                }],
                explicit: false,
                complete_table_names: false,
            },
            SqlCompletionState::default(),
        );
        assert!(s.is_open(), "popup should open behind `SELECT `");
        assert!(
            s.items.iter().any(|i| i.label == "status"),
            "columns should be offered behind `SELECT `, got: {:?}",
            s.items
        );
    }

    #[test]
    fn select_list_cursor_after_space_offers_columns() {
        // `select <cursor> from t1 t` — cursor right after `select ` — must
        // offer the t1 columns even though the cursor sits after a space.
        let (s, _i, _e, _d) = update(
            SqlCompletionMessage::Refresh {
                sql: "select from t1 t".into(),
                cursor: cursor_at("select "),
                tables: vec!["t1".into()],
                columns: vec![
                    ColumnInfo {
                        name: "id".into(),
                        type_name: "integer".into(),
                        type_display: "integer".into(),
                        comment: None,
                    },
                    ColumnInfo {
                        name: "title".into(),
                        type_name: "text".into(),
                        type_display: "text".into(),
                        comment: None,
                    },
                ],
                explicit: false,
                complete_table_names: false,
            },
            SqlCompletionState::default(),
        );
        assert!(
            s.is_open(),
            "popup should open at `select ` in a column list"
        );
        assert!(
            s.items.iter().any(|i| i.label == "title"),
            "t1 columns should be offered, got: {:?}",
            s.items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn close_clears_items() {
        let (mut s, _i, _e, _d) = update(
            SqlCompletionMessage::Refresh {
                sql: "sel".into(),
                cursor: cursor_at("sel"),
                tables: vec![],
                columns: vec![],
                explicit: false,
                complete_table_names: false,
            },
            SqlCompletionState::default(),
        );
        assert!(s.is_open());
        let (s2, _i, _e, _d) = update(SqlCompletionMessage::Close, s);
        s = s2;
        assert!(!s.is_open());
        assert!(s.items.is_empty());
    }
}
