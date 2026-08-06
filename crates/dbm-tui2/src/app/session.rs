//! TUI session persistence: save/restore the open SQL tabs and their context
//! across app restarts.
//!
//! The on-disk schema lives in `dbm_store` (`TuiSessionSnapshot`, stored as
//! JSON under `tui_session`). This module adapts the current `AppState` to that
//! schema. Fields the shell does not yet track (split ratios, tree width, etc.)
//! are persisted as defaults and round-trip harmlessly; the schema is additive
//! so older/newer snapshots remain compatible as the shell grows.

use dbm_store::{
    TUI_SESSION_VERSION, TuiSessionSnapshot, TuiTabSnapshot, TuiTreeSnapshot,
    load_tui_session, save_tui_session,
};

use crate::app::state::AppState;
use crate::app_shell::focus::FocusZone;

/// Restore a previously persisted session into `state`. Returns `Ok(false)`
/// when no compatible snapshot exists (fresh start).
pub fn restore_session(state: &mut AppState) -> anyhow::Result<bool> {
    let Some(snapshot) = load_tui_session()? else {
        return Ok(false);
    };
    apply_snapshot(state, &snapshot);
    Ok(true)
}

/// Persist the current session. Best-effort: a failure to write is surfaced to
/// the caller (which may log or ignore it at exit).
pub fn persist_session(state: &AppState) -> anyhow::Result<()> {
    save_tui_session(&snapshot_from_app(state))
        .map_err(|e| anyhow::anyhow!("failed to save TUI session: {e}"))
}

fn snapshot_from_app(state: &AppState) -> TuiSessionSnapshot {
    let tabs = state
        .sql
        .sql_tab
        .tabs
        .iter()
        .enumerate()
        .map(|(idx, tab)| {
            let session = &tab.session;
            let sql = crate::common::editor::editor_text(&tab.editor.editor);
            TuiTabSnapshot {
                instance: session.instance.clone().unwrap_or_default(),
                connection: session.connection.clone().unwrap_or_default(),
                sequence: idx as u32,
                sql,
                split_ratio: 45,
                history_pane_width: 24,
                detail_pane_width: 32,
                database: session.database.clone().unwrap_or_default(),
                schema: session.schema.clone().unwrap_or_default(),
                complete_table_names: false,
            }
        })
        .collect();

    TuiSessionSnapshot {
        version: TUI_SESSION_VERSION,
        focus: focus_name(state.focus).to_string(),
        tree_width: 20,
        tree: TuiTreeSnapshot {
            expanded_instances: Vec::new(),
            cursor: None,
            active_workspace: None,
        },
        tabs,
        active_tab: Some(state.sql.sql_tab.active_tab),
        instance_workspace: None,
        discover_targets_ratio: 35,
        explorer_split_ratio: 20,
        explorer_pane: "instances".into(),
    }
}

fn apply_snapshot(state: &mut AppState, snapshot: &TuiSessionSnapshot) {
    use crate::features::sql_workspace::sql_tab::editor::state::EditorState;

    // Rebuild the tab list from the snapshot.
    let tabs: Vec<crate::features::sql_workspace::sql_tab::state::SqlTab> = snapshot
        .tabs
        .iter()
        .map(|t| crate::features::sql_workspace::sql_tab::state::SqlTab {
            session: crate::features::sql_workspace::sql_tab::session::TabSession {
                id: t.sequence as usize,
                instance: non_empty(t.instance.clone()),
                connection: non_empty(t.connection.clone()),
                connection_id: None,
                database: non_empty(t.database.clone()),
                schema: non_empty(t.schema.clone()),
            },
            focus: crate::features::sql_workspace::sql_tab::state::SqlFocus::default(),
            editor: EditorState::with_sql(&t.sql),
            results: crate::features::sql_workspace::sql_tab::results::state::ResultsState::new(),
            history: crate::features::sql_workspace::sql_tab::history::state::HistoryState::default(),
        })
        .collect();

    // Replace the default single tab with the restored set. When there are no
    // persisted tabs, keep the default empty tab.
    if !tabs.is_empty() {
        let active = snapshot
            .active_tab
            .filter(|&i| i < tabs.len())
            .unwrap_or(0);
        state.sql.sql_tab.tabs = tabs;
        state.sql.sql_tab.active_tab = active;
    }

    state.focus = focus_from_name(&snapshot.focus);
}

fn non_empty(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

fn focus_name(focus: FocusZone) -> &'static str {
    match focus {
        FocusZone::Header => "header",
        FocusZone::Explorer => "explorer",
        FocusZone::SQLWorkspace | FocusZone::InstanceWorkspace => "workspace",
    }
}

fn focus_from_name(name: &str) -> FocusZone {
    match name {
        "header" => FocusZone::Header,
        "workspace" => FocusZone::SQLWorkspace,
        "explorer" | "tree" => FocusZone::Explorer,
        _ => FocusZone::Header,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> AppState {
        AppState::default()
    }

    #[test]
    fn snapshot_round_trips_default_state() {
        let state = sample_state();
        let snap = snapshot_from_app(&state);
        assert_eq!(snap.version, TUI_SESSION_VERSION);
        assert_eq!(snap.focus, "header");
        // Default app opens one empty tab.
        assert_eq!(snap.tabs.len(), 1);
        assert!(snap.tabs[0].sql.is_empty());
    }

    #[test]
    fn restore_replaces_tabs_from_snapshot() {
        let snap = TuiSessionSnapshot {
            version: TUI_SESSION_VERSION,
            focus: "workspace".into(),
            tree_width: 20,
            tree: TuiTreeSnapshot {
                expanded_instances: Vec::new(),
                cursor: None,
                active_workspace: None,
            },
            tabs: vec![
                TuiTabSnapshot {
                    instance: "local".into(),
                    connection: "app".into(),
                    sequence: 0,
                    sql: "select 1".into(),
                    split_ratio: 45,
                    history_pane_width: 24,
                    detail_pane_width: 32,
                    database: "mydb".into(),
                    schema: "public".into(),
                    complete_table_names: false,
                },
                TuiTabSnapshot {
                    instance: "local".into(),
                    connection: "app".into(),
                    sequence: 1,
                    sql: "select 2".into(),
                    split_ratio: 45,
                    history_pane_width: 24,
                    detail_pane_width: 32,
                    database: "mydb".into(),
                    schema: "public".into(),
                    complete_table_names: false,
                },
            ],
            active_tab: Some(1),
            instance_workspace: None,
            discover_targets_ratio: 35,
            explorer_split_ratio: 20,
            explorer_pane: "instances".into(),
        };
        let mut state = sample_state();
        apply_snapshot(&mut state, &snap);
        assert_eq!(state.sql.sql_tab.tabs.len(), 2);
        assert_eq!(state.sql.sql_tab.active_tab, 1);
        assert_eq!(
            crate::common::editor::editor_text(&state.sql.sql_tab.tabs[1].editor.editor),
            "select 2"
        );
        assert_eq!(
            state.sql.sql_tab.tabs[0].session.instance.as_deref(),
            Some("local")
        );
        assert_eq!(state.focus, FocusZone::SQLWorkspace);

        // Sanity: focus round-trips through snapshot_from_app.
        let back = snapshot_from_app(&state);
        assert_eq!(back.focus, "workspace");
        assert_eq!(back.tabs.len(), 2);
    }

    #[test]
    fn empty_tabs_keep_default_tab() {
        let snap = TuiSessionSnapshot {
            version: TUI_SESSION_VERSION,
            focus: "explorer".into(),
            tree_width: 20,
            tree: TuiTreeSnapshot {
                expanded_instances: Vec::new(),
                cursor: None,
                active_workspace: None,
            },
            tabs: Vec::new(),
            active_tab: None,
            instance_workspace: None,
            discover_targets_ratio: 35,
            explorer_split_ratio: 20,
            explorer_pane: "instances".into(),
        };
        let mut state = sample_state();
        apply_snapshot(&mut state, &snap);
        // One empty default tab remains.
        assert_eq!(state.sql.sql_tab.tabs.len(), 1);
        assert_eq!(state.focus, FocusZone::Explorer);
    }
}
