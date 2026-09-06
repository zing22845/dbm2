//! Update the `sql_tab` state, delegating to child modules.
//!
//! The message set is grouped by concern; each cluster owns its own
//! `match msg { .. }` in a child module and writes its results into a shared
//! [`SqlTabOut`]. Only this module's [`update`] is public.

pub mod delegate;
pub mod query;
pub mod splitter;
pub mod tabs;

use super::effect::SqlTabEffect;
use super::history;
use super::intent::SqlTabIntent;
use super::msg::SqlTabMessage;
use super::state::SqlTabState;

/// Accumulators a single update pass collects on its way through the clusters.
pub(super) struct SqlTabOut {
    pub intents: Vec<SqlTabIntent>,
    pub effects: Vec<SqlTabEffect>,
    pub dirty: bool,
}

/// Derive the `(instance, connection)` key from a tab's session, falling back
/// to the numeric `connection_id` when the display names are not yet bound.
pub(super) fn session_key(session: &super::session::TabSession) -> (String, String) {
    let instance = session.instance.clone().unwrap_or_default();
    let connection = session
        .connection
        .clone()
        .or_else(|| session.connection_id.clone())
        .unwrap_or_default();
    (instance, connection)
}
/// Log when a routed child message targets a `tab_id` that no longer exists
/// (e.g. its tab was closed). The message is dropped; this is expected for
/// late async results, but worth surfacing so a stale tab id isn't silently
/// swallowed forever.
pub(super) fn warn_tab_missing(tab_id: usize) {
    tracing::warn!("sql_tab: message targeted a missing tab (tab_id = {tab_id}); dropped");
}
/// When the History detail is visible the History zone is `list + detail +
/// splitter`, capped at `max_zone_w = area.width - MIN_SQL_PANE_WIDTH` (the
/// editor keeps its min width). A list width that was legal while History was
/// unfocused (up to `history_max`) overflows once the detail appears, so the
/// *rendered* list gets squeezed below its stored width (the zone clamps).
/// Re-clamp the stored list to `history_max - detail` so storage and rendered
/// geometry stay identical (no redundant repaints at the drag limit). Returns
/// `true` when the stored width changed.
pub(super) fn clamp_list_for_history_detail(
    tab: &mut super::state::SqlTab,
    store: &history::store::SqlHistoryStore,
) -> bool {
    use crate::features::sql_workspace::sql_tab::splitter::state::{
        MAX_HISTORY_WIDTH, MIN_HISTORY_WIDTH,
    };
    let (instance, connection) = session_key(&tab.session);
    if tab.focus != crate::features::sql_workspace::sql_tab::state::SqlFocus::History
        || store.entries(&instance, &connection).is_empty()
    {
        return false;
    }
    let hi = tab
        .splitter
        .history_max
        .min(MAX_HISTORY_WIDTH)
        .saturating_sub(tab.history.splitter.detail_pane_width)
        .saturating_sub(2); // History border the detail pane takes over
    if tab.splitter.history_pane_width <= hi {
        return false;
    }
    let lo = tab.splitter.history_min.max(MIN_HISTORY_WIDTH);
    // When the detail pane leaves no room between the list minimum and the
    // history maximum the constraint is unsatisfiable; leave the stored width
    // alone (the rendered geometry already clamps) instead of panicking in
    // `clamp` when `lo > hi`.
    if lo > hi {
        return false;
    }
    tab.splitter.history_pane_width = tab.splitter.history_pane_width.clamp(lo, hi);
    true
}

/// Update the `sql_tab` state, delegating to child modules.
///
/// The returned `bool` is `dirty`: whether any rendered tab state changed.
/// Messages routed to a missing tab (logged and dropped) report `false`.
pub fn update(
    msg: SqlTabMessage,
    mut state: SqlTabState,
) -> (SqlTabState, Vec<SqlTabIntent>, Vec<SqlTabEffect>, bool) {
    let mut out = SqlTabOut {
        intents: Vec::new(),
        effects: Vec::new(),
        dirty: false,
    };
    match msg {
        SqlTabMessage::Tab(..)
        | SqlTabMessage::Focus(..)
        | SqlTabMessage::OpenTab
        | SqlTabMessage::CloseTab(..)
        | SqlTabMessage::OpenConnectionTab { .. }
        | SqlTabMessage::FocusConnectionTab { .. }
        | SqlTabMessage::SetActiveConnection { .. }
        | SqlTabMessage::ApplyContext { .. } => tabs::apply(msg, &mut state, &mut out),
        SqlTabMessage::SetEditorTopHeight { .. }
        | SqlTabMessage::NudgeEditorTopHeight { .. }
        | SqlTabMessage::SetHistoryWidth { .. }
        | SqlTabMessage::SetHistoryStore { .. }
        | SqlTabMessage::SetHistoryDetailWidth { .. }
        | SqlTabMessage::NudgeHistoryDetailWidth { .. }
        | SqlTabMessage::SetResultsDetailWidth { .. }
        | SqlTabMessage::NudgeResultsDetailWidth { .. }
        | SqlTabMessage::NudgeHistoryWidth { .. } => splitter::apply(msg, &mut state, &mut out),
        SqlTabMessage::RecallHistory { .. }
        | SqlTabMessage::EnterHistoryRecall { .. }
        | SqlTabMessage::RunQueryFromEditor { .. }
        | SqlTabMessage::RunTableQuery { .. }
        | SqlTabMessage::ToggleTableCompletion { .. }
        | SqlTabMessage::ReloadCompletionCatalog { .. } => query::apply(msg, &mut state, &mut out),
        SqlTabMessage::Editor { .. }
        | SqlTabMessage::Results { .. }
        | SqlTabMessage::History { .. } => delegate::apply(msg, &mut state, &mut out),
    }
    (state, out.intents, out.effects, out.dirty)
}

#[cfg(test)]
mod tests {
    use super::super::editor;
    use super::*;
    use crate::features::sql_workspace::sql_tab::update::update;

    #[test]
    fn tblcmp_on_after_from_offers_table_names() {
        use crate::features::sql_workspace::sql_tab::editor::state::CompletionCatalog;
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

        let char_key = |c: char| KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        s.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;
        // Seed the table-name catalog so the popup has tables to offer.
        s.tabs[0].editor.completion_catalog = CompletionCatalog {
            tables: vec!["users".into(), "orders".into()],
            columns_by_table: Default::default(),
        };

        // Alt+Tab enables TblCmp.
        let (mut s, _i, _e, _d) = update(SqlTabMessage::ToggleTableCompletion { tab_id }, s);
        assert!(s.tabs[0].complete_table_names);
        assert!(s.tabs[0].editor.complete_table_names);

        // Type `select * from ` and confirm the table popup appears.
        for c in "select * from ".chars() {
            let (s2, _i, _e, _d) = update(
                SqlTabMessage::Editor {
                    tab_id,
                    msg: editor::msg::EditorMsg::Message(editor::msg::EditorMessage::KeyEvent {
                        key: char_key(c),
                        tracked_caps_lock: false,
                    }),
                },
                s,
            );
            s = s2;
        }
        let items = &s.tabs[0].editor.sql_completion.items;
        assert!(
            s.tabs[0].editor.sql_completion.is_open(),
            "popup should open after `from `"
        );
        assert!(
            items.iter().any(|i| i.label == "users"),
            "table names must be offered with TblCmp on, got: {items:?}"
        );
    }
    #[test]
    fn run_query_from_editor_clears_editor_on_success() {
        use super::super::results::msg::{ResultsMessage, ResultsMsg};
        use super::super::results::state::QueryResultData;

        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        crate::common::editor::set_sql_text(&mut s.tabs[0].editor.editor, "select 1");
        s.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;

        // An editor-initiated run arms the transient clear flag.
        let (s, _i, _e, _d) = update(
            SqlTabMessage::RunQueryFromEditor {
                tab_id,
                sql: "select 1".into(),
            },
            s,
        );
        assert!(
            s.tabs[0].clear_editor_after_run,
            "run from editor must arm the clear flag"
        );

        // On success the editor is emptied and the flag dropped.
        let (s, _i2, _e2, _d2) = update(
            SqlTabMessage::Results {
                tab_id,
                msg: ResultsMsg::Message(ResultsMessage::SetResult {
                    result: QueryResultData {
                        columns: vec![],
                        rows: vec![vec!["1".into()]],
                        rows_affected: None,
                        total_rows: None,
                    },
                    paginated: true,
                }),
            },
            s,
        );
        assert!(
            !s.tabs[0].clear_editor_after_run,
            "clear flag must reset after a successful run"
        );
        assert_eq!(
            crate::common::editor::editor_text(&s.tabs[0].editor.editor),
            "",
            "editor must be cleared after a successful editor-run query"
        );
        assert_eq!(
            s.tabs[0].editor.editor.mode,
            edtui::EditorMode::Insert,
            "editor must return to Insert after a successful run"
        );
    }
    #[test]
    fn run_query_from_editor_preserves_buffer_on_failure() {
        use super::super::results::msg::{ResultsMessage, ResultsMsg};

        let mut s = SqlTabState::default();
        s.open_connection_tab("inst".into(), "c1".into(), "id1".into(), None, None, None);
        let tab_id = s.tabs[0].session.id;
        crate::common::editor::set_sql_text(&mut s.tabs[0].editor.editor, "select 1");
        s.tabs[0].editor.editor.mode = edtui::EditorMode::Insert;

        let (s, _i, _e, _d) = update(
            SqlTabMessage::RunQueryFromEditor {
                tab_id,
                sql: "select 1".into(),
            },
            s,
        );
        assert!(s.tabs[0].clear_editor_after_run);

        // On failure the buffer is preserved for editing and the flag dropped.
        let (s, _i2, _e2, _d2) = update(
            SqlTabMessage::Results {
                tab_id,
                msg: ResultsMsg::Message(ResultsMessage::QueryError {
                    message: "boom".into(),
                }),
            },
            s,
        );
        assert!(
            !s.tabs[0].clear_editor_after_run,
            "clear flag must reset when the run fails"
        );
        assert_eq!(
            crate::common::editor::editor_text(&s.tabs[0].editor.editor),
            "select 1",
            "a failed editor-run query must keep its text for editing"
        );
    }
}
