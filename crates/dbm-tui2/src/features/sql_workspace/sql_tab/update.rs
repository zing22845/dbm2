//! `sql_tab` feature update.
//!
//! This update is a pure transition: it takes the tab state by value and
//! returns a new one, moving (not cloning) the sub-states it touches. Child
//! messages carry a `tab_id` and are routed to that specific tab, so a child
//! update only ever moves the targeted tab's sub-state out and back — the
//! other tabs are untouched, and no `O(total tabs)` deep clone happens per
//! message. Intents/effects are re-tagged with the same `tab_id` so the
//! cascade lands back on the originating tab.

use super::msg::SqlTabMessage;
use super::state::SqlTabState;
use super::intent::SqlTabIntent;
use super::effect::SqlTabEffect;
use super::editor;
use super::history;
use super::results;

/// Update the `sql_tab` state, delegating to child modules.
pub fn update(
    msg: SqlTabMessage,
    mut state: SqlTabState,
) -> (SqlTabState, Vec<SqlTabIntent>, Vec<SqlTabEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    match msg {
        SqlTabMessage::Tab(idx) => {
            state.active_tab = idx.min(state.tabs.len().saturating_sub(1));
        }
        SqlTabMessage::OpenTab => state.open_tab(),
        SqlTabMessage::CloseTab(idx) => state.close_tab(idx),
        SqlTabMessage::Editor { tab_id, msg } => {
            let editor::msg::EditorMsg::Message(inner) = msg;
            if let Some(idx) = state.index_of(tab_id) {
                let editor_state = std::mem::take(&mut state.tabs[idx].editor);
                let (s, i, e) = editor::update::update(inner, editor_state);
                state.tabs[idx].editor = s;
                intents.extend(
                    i.into_iter()
                        .map(|intent| SqlTabIntent::Editor { tab_id, intent }),
                );
                effects.extend(
                    e.into_iter()
                        .map(|effect| SqlTabEffect::Editor { tab_id, effect }),
                );
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::Results { tab_id, msg } => {
            let results::msg::ResultsMsg::Message(inner) = msg;
            if let Some(idx) = state.index_of(tab_id) {
                let results_state = std::mem::take(&mut state.tabs[idx].results);
                let (s, i, e) = results::update::update(inner, results_state);
                state.tabs[idx].results = s;
                intents.extend(
                    i.into_iter()
                        .map(|intent| SqlTabIntent::Results { tab_id, intent }),
                );
                effects.extend(
                    e.into_iter()
                        .map(|effect| SqlTabEffect::Results { tab_id, effect }),
                );
            } else {
                warn_tab_missing(tab_id);
            }
        }
        SqlTabMessage::History { tab_id, msg } => {
            let history::msg::HistoryMsg::Message(inner) = msg;
            if let Some(idx) = state.index_of(tab_id) {
                let history_state = std::mem::take(&mut state.tabs[idx].history);
                let (s, i, e) = history::update::update(inner, history_state);
                state.tabs[idx].history = s;
                intents.extend(
                    i.into_iter()
                        .map(|intent| SqlTabIntent::History { tab_id, intent }),
                );
                effects.extend(
                    e.into_iter()
                        .map(|effect| SqlTabEffect::History { tab_id, effect }),
                );
            } else {
                warn_tab_missing(tab_id);
            }
        }
    }
    (state, intents, effects)
}

/// Log when a routed child message targets a `tab_id` that no longer exists
/// (e.g. its tab was closed). The message is dropped; this is expected for
/// late async results, but worth surfacing so a stale tab id isn't silently
/// swallowed forever.
fn warn_tab_missing(tab_id: usize) {
    tracing::warn!("sql_tab: message targeted a missing tab (tab_id = {tab_id}); dropped");
}
