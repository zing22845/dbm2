//! TUI session persistence: save/restore the open SQL tabs and their context
//! across app restarts.
//!
//! The on-disk schema lives in `dbm_store` (`TuiSessionSnapshot`, stored as
//! JSON under `tui_session`). The children split the two directions:
//! [`snapshot`] collects the current state into that snapshot and [`restore`]
//! applies a snapshot back onto the state.

pub mod restore;
pub mod snapshot;

use self::restore::apply_snapshot;
use self::snapshot::snapshot_from_app;
use dbm_store::{load_tui_session, save_tui_session};

use crate::app::action::Action;
use crate::app::state::AppState;
use crate::app_shell::effect::ErasedEffect;

/// Restore a previously persisted session into `state`. Returns the
/// `LoadConnections` effects needed to fetch the subtrees of instances that
/// were restored expanded (their connections are loaded lazily). Returns an
/// empty vec when no compatible snapshot exists (fresh start).
pub fn restore_session(state: &mut AppState) -> anyhow::Result<Vec<Box<dyn ErasedEffect<Action>>>> {
    // Load the persisted SQL history once at startup and seed it into every
    // restored tab, mirroring the original dbm's `load_sql_history` (the store
    // groups by `(instance, connection)`, so each tab sees the history for any
    // connection it later binds to).
    let loaded = dbm_store::Store::open_default()
        .ok()
        .and_then(|store| store.load_sql_history().ok());
    let effects = if let Some(snapshot) = load_tui_session()? {
        apply_snapshot(state, &snapshot)
    } else {
        Vec::new()
    };
    if let Some(map) = loaded {
        state.sql.sql_tab.history_store =
            crate::features::sql_workspace::sql_tab::history::store::SqlHistoryStore::from_map(map);
    }
    Ok(effects)
}
/// Persist the current session. Best-effort: a failure to write is surfaced to
/// the caller (which may log or ignore it at exit).
pub fn persist_session(state: &AppState) -> anyhow::Result<()> {
    save_tui_session(&snapshot_from_app(state))
        .map_err(|e| anyhow::anyhow!("failed to save TUI session: {e}"))
}

#[cfg(test)]
mod tests {
    use super::snapshot::{approx_discover_body_height, approx_explorer_body_height};
    use super::*;
    use dbm_store::TUI_SESSION_VERSION;

    fn sample_state() -> AppState {
        AppState::default()
    }

    #[test]
    fn snapshot_round_trips_default_state() {
        let state = sample_state();
        let snap = snapshot_from_app(&state);
        assert_eq!(snap.version, TUI_SESSION_VERSION);
        assert_eq!(snap.focus, "header");
        // Default app opens no tabs (a tab is opened when a connection is
        // selected in the tree).
        assert!(snap.tabs.is_empty());
    }

    #[test]
    fn snapshot_persists_and_restores_explorer_instances_split() {
        // The explorer instances/objects split is persisted as a percentage and
        // re-materialized to rows against the current body height on restore.
        let mut state = sample_state();
        state.term_height = 48; // explorer body ≈ 40 rows
        state
            .explorer
            .splitter
            .set_instances_height_pct(50, approx_explorer_body_height(&state));
        let snap = snapshot_from_app(&state);
        assert_eq!(snap.explorer_split_ratio, 50);

        let mut restored = sample_state();
        restored.term_height = 48;
        let effects = apply_snapshot(&mut restored, &snap);
        let track = approx_explorer_body_height(&restored);
        assert_eq!(
            restored.explorer.splitter.instances_height,
            (track * 50) / 100
        );
        assert_eq!(restored.explorer.splitter.instances_height_pct(track), 50);
        drop(effects);
    }

    #[test]
    fn snapshot_persists_and_restores_explorer_pane_width() {
        // The Explorer / workspace splitter width is persisted in `tree_width`
        // and clamped on restore.
        let mut state = sample_state();
        state.splitter.explorer_pane_width = 40;
        let snap = snapshot_from_app(&state);
        assert_eq!(snap.tree_width, 40);

        // An out-of-range persisted width is clamped on restore.
        let mut restored = sample_state();
        let mut s = snap.clone();
        s.tree_width = 9999;
        let effects = apply_snapshot(&mut restored, &s);
        assert!(
            restored.splitter.explorer_pane_width
                == crate::features::app_splitter::state::MAX_EXPLORER_WIDTH
        );
        // A valid width restores exactly.
        let mut restored2 = sample_state();
        apply_snapshot(&mut restored2, &snap);
        assert_eq!(restored2.splitter.explorer_pane_width, 40);
        drop(effects);
    }

    #[test]
    fn snapshot_persists_and_restores_discover_targets_split() {
        // The discover targets/results split is persisted as a percentage and
        // re-materialized to rows against the current body height on restore.
        let mut state = sample_state();
        state.term_height = 46; // discover body ≈ 24 rows
        state
            .discover
            .splitter
            .set_targets_height_pct(50, approx_discover_body_height(&state));
        let snap = snapshot_from_app(&state);
        assert_eq!(snap.discover_targets_ratio, 50);

        let mut restored = sample_state();
        restored.term_height = 46;
        let effects = apply_snapshot(&mut restored, &snap);
        let track = approx_discover_body_height(&restored);
        assert_eq!(
            restored.discover.splitter.targets_height,
            (track * 50) / 100
        );
        assert_eq!(restored.discover.splitter.targets_height_pct(track), 50);
        drop(effects);
    }
}
